//! Document-level properties: custom and application properties, view and
//! compatibility settings, document protection and the page background.

use std::hash::{BuildHasher, Hasher};

use openxml_core::part::{read_part, write_part};
use openxml_core::{Error, Length, Result};
use openxml_opc::PartName;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::shared_custom_properties as cp;
use openxml_schema::shared_extended_properties as ep;
use openxml_schema::shared_types::{ST_AlgClass, ST_AlgType, ST_CryptProv, ST_OnOff};
use openxml_schema::wml::{self, ST_DocProtect};
use openxml_xml::Base64Binary;

use crate::document::Document;
use crate::util::{hex_color, hex_color_string, is_on, on, twips, twips_value};

/// Format id of user-defined custom properties.
const CUSTOM_FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";
const COMPAT_URI: &str = "http://schemas.microsoft.com/office/word";
/// SHA-1 (`cryptAlgorithmSid` 4) iterations used for new password hashes.
const SPIN_COUNT: u32 = 100_000;

/// Value of a custom document property.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    /// Text (`vt:lpwstr`).
    Text(String),
    /// Integer (`vt:i4`, or `vt:i8` when it does not fit).
    Integer(i64),
    /// Floating-point number (`vt:r8`).
    Number(f64),
    /// Yes/no (`vt:bool`).
    Bool(bool),
    /// Date and time, `YYYY-MM-DDThh:mm:ssZ` (`vt:filetime`).
    Date(String),
}

impl PropertyValue {
    fn to_choice(&self) -> cp::CT_Property_Choice {
        match self {
            PropertyValue::Text(s) => cp::CT_Property_Choice::Lpwstr(s.clone()),
            PropertyValue::Integer(i) => match i32::try_from(*i) {
                Ok(v) => cp::CT_Property_Choice::I4(v),
                Err(_) => cp::CT_Property_Choice::I8(*i),
            },
            PropertyValue::Number(n) => cp::CT_Property_Choice::R8(*n),
            PropertyValue::Bool(b) => cp::CT_Property_Choice::Bool(*b),
            PropertyValue::Date(d) => cp::CT_Property_Choice::Filetime(d.clone()),
        }
    }

    fn from_choice(c: &cp::CT_Property_Choice) -> Option<Self> {
        use cp::CT_Property_Choice as C;
        Some(match c {
            C::Lpwstr(s) | C::Lpstr(s) | C::Bstr(s) => PropertyValue::Text(s.clone()),
            C::I1(v) => PropertyValue::Integer(i64::from(*v)),
            C::I2(v) => PropertyValue::Integer(i64::from(*v)),
            C::I4(v) | C::Int(v) => PropertyValue::Integer(i64::from(*v)),
            C::I8(v) => PropertyValue::Integer(*v),
            C::Ui1(v) => PropertyValue::Integer(i64::from(*v)),
            C::Ui2(v) => PropertyValue::Integer(i64::from(*v)),
            C::Ui4(v) | C::Uint(v) => PropertyValue::Integer(i64::from(*v)),
            C::R4(v) => PropertyValue::Number(f64::from(*v)),
            C::R8(v) | C::Decimal(v) => PropertyValue::Number(*v),
            C::Bool(b) => PropertyValue::Bool(*b),
            C::Filetime(d) | C::Date(d) => PropertyValue::Date(d.clone()),
            _ => return None,
        })
    }
}

/// Kind of editing restriction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protection {
    /// No changes.
    ReadOnly,
    /// Comments only.
    Comments,
    /// Changes are tracked.
    TrackedChanges,
    /// Form fields only.
    Forms,
}

// ----- SHA-1 (FIPS 180-4), used by the password hash -------------------------------------

/// SHA-1 digest of `data`.
pub(crate) fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (chunk, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *chunk = v.to_be_bytes();
    }
    out
}

// ----- legacy password key (ECMA-376 Part 4 §14.8.1) -------------------------------------

const INITIAL_CODE: [u16; 15] = [
    0xE1F0, 0x1D0F, 0xCC9C, 0x84C0, 0x110C, 0x0E10, 0xF1CE, 0x313E, 0x1872, 0xE139, 0xD40F, 0x84F9, 0x280C,
    0xA96A, 0x4EC3,
];

/// Encryption matrix rows, from the row used for the 15th character from
/// the end ("last − 14") down to the last character.
const MATRIX: [[u16; 7]; 15] = [
    [0xAEFC, 0x4DD9, 0x9BB2, 0x2745, 0x4E8A, 0x9D14, 0x2A09],
    [0x7B61, 0xF6C2, 0xFDA5, 0xEB6B, 0xC6F7, 0x9DCF, 0x2BBF],
    [0x4563, 0x8AC6, 0x05AD, 0x0B5A, 0x16B4, 0x2D68, 0x5AD0],
    [0x0375, 0x06EA, 0x0DD4, 0x1BA8, 0x3750, 0x6EA0, 0xDD40],
    [0xD849, 0xA0B3, 0x5147, 0xA28E, 0x553D, 0xAA7A, 0x44D5],
    [0x6F45, 0xDE8A, 0xAD35, 0x4A4B, 0x9496, 0x390D, 0x721A],
    [0xEB23, 0xC667, 0x9CEF, 0x29FF, 0x53FE, 0xA7FC, 0x5FD9],
    [0x47D3, 0x8FA6, 0x0F6D, 0x1EDA, 0x3DB4, 0x7B68, 0xF6D0],
    [0xB861, 0x60E3, 0xC1C6, 0x93AD, 0x377B, 0x6EF6, 0xDDEC],
    [0x45A0, 0x8B40, 0x06A1, 0x0D42, 0x1A84, 0x3508, 0x6A10],
    [0xAA51, 0x4483, 0x8906, 0x022D, 0x045A, 0x08B4, 0x1168],
    [0x76B4, 0xED68, 0xCAF1, 0x85C3, 0x1BA7, 0x374E, 0x6E9C],
    [0x3730, 0x6E60, 0xDCC0, 0xA9A1, 0x4363, 0x86C6, 0x1DAD],
    [0x3331, 0x6662, 0xCCC4, 0x89A9, 0x0373, 0x06E6, 0x0DCC],
    [0x1021, 0x2042, 0x4084, 0x8108, 0x1231, 0x2462, 0x48C4],
];

/// The 32-bit legacy key of a password (Part 4 §14.8.1).
pub(crate) fn legacy_key(password: &str) -> u32 {
    let bytes: Vec<u8> = password
        .encode_utf16()
        .take(15)
        .map(|c| {
            let low = (c & 0xFF) as u8;
            if low != 0 { low } else { (c >> 8) as u8 }
        })
        .collect();
    if bytes.is_empty() {
        return 0;
    }
    let len = bytes.len();
    let mut high = INITIAL_CODE[len - 1];
    for (i, &c) in bytes.iter().enumerate() {
        // The last character uses the last row, the one before it the row above, …
        let row = &MATRIX[15 - len + i];
        for (bit, value) in row.iter().enumerate() {
            if c & (1 << bit) != 0 {
                high ^= value;
            }
        }
    }
    let f = |x: u16| ((x >> 14) & 1) | ((x << 1) & 0x7FFF);
    let mut low: u16 = 0;
    for &c in bytes.iter().rev() {
        low = f(low) ^ u16::from(c);
    }
    low = f(low) ^ (len as u16) ^ 0xCE4B;
    (u32::from(high) << 16) | u32::from(low)
}

/// The password hash stored in `w:documentProtection`: SHA-1 over the salt
/// and the legacy key (in reversed byte order), then `spin_count`
/// iterations over the previous hash and the little-endian iteration number.
pub(crate) fn protection_hash(password: &str, salt: &[u8], spin_count: u32) -> [u8; 20] {
    let key = legacy_key(password).to_le_bytes();
    let mut input = salt.to_vec();
    input.extend_from_slice(&key);
    let mut hash = sha1(&input);
    for i in 0..spin_count {
        let mut buf = hash.to_vec();
        buf.extend_from_slice(&i.to_le_bytes());
        hash = sha1(&buf);
    }
    hash
}

fn random_salt() -> [u8; 16] {
    let mut out = [0u8; 16];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_usize(i);
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    out
}

impl Document {
    // ----- custom properties ------------------------------------------------------------

    fn custom_part(&self) -> Option<PartName> {
        self.shared
            .package
            .related_part(None, rel_types::CUSTOM_PROPERTIES)
            .filter(|p| self.shared.package.contains(p))
    }

    fn read_custom(&self) -> Result<cp::CT_Properties> {
        match self.custom_part() {
            Some(name) => read_part(&self.shared.package, &name, &cp::elements::PROPERTIES),
            None => Ok(cp::CT_Properties::default()),
        }
    }

    fn write_custom(&mut self, props: &cp::CT_Properties) -> Result<()> {
        let name = match self.custom_part() {
            Some(name) => name,
            None => {
                let name = PartName::new("/docProps/custom.xml")?;
                self.shared
                    .package
                    .add_part(name.clone(), ct::CUSTOM_PROPERTIES, Vec::new())?;
                self.shared
                    .package
                    .add_relationship(None, rel_types::CUSTOM_PROPERTIES, &name)?;
                name
            }
        };
        write_part(
            &mut self.shared.package,
            &name,
            ct::CUSTOM_PROPERTIES,
            &cp::elements::PROPERTIES,
            props,
        )
    }

    /// Custom document properties (`docProps/custom.xml`) with a supported value type.
    ///
    /// ```
    /// use openxml_docx::{Document, PropertyValue};
    ///
    /// let mut doc = Document::new();
    /// doc.set_custom_property("Project", PropertyValue::Text("Apollo".into()))?;
    /// doc.set_custom_property("Budget", PropertyValue::Number(1.5e6))?;
    /// assert_eq!(doc.custom_property("Project")?, Some(PropertyValue::Text("Apollo".into())));
    /// assert_eq!(doc.custom_properties()?.len(), 2);
    /// # Ok::<(), openxml_docx::Error>(())
    /// ```
    pub fn custom_properties(&self) -> Result<Vec<(String, PropertyValue)>> {
        Ok(self
            .read_custom()?
            .property
            .iter()
            .filter_map(|p| Some((p.name.clone()?, PropertyValue::from_choice(p.choice.as_ref()?)?)))
            .collect())
    }

    /// A custom property by name.
    pub fn custom_property(&self, name: &str) -> Result<Option<PropertyValue>> {
        Ok(self
            .custom_properties()?
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v))
    }

    /// Adds or replaces a custom property.
    pub fn set_custom_property(&mut self, name: &str, value: PropertyValue) -> Result<()> {
        if name.is_empty() {
            return Err(Error::InvalidArgument("a custom property needs a name".into()));
        }
        let mut props = self.read_custom()?;
        match props
            .property
            .iter_mut()
            .find(|p| p.name.as_deref() == Some(name))
        {
            Some(p) => p.choice = Some(value.to_choice()),
            None => {
                // Property ids start at 2 (0 and 1 are reserved).
                let pid = props
                    .property
                    .iter()
                    .filter_map(|p| p.pid)
                    .max()
                    .unwrap_or(1)
                    .max(1)
                    + 1;
                props.property.push(cp::CT_Property {
                    fmtid: Some(CUSTOM_FMTID.into()),
                    pid: Some(pid),
                    name: Some(name.to_owned()),
                    choice: Some(value.to_choice()),
                    ..Default::default()
                });
            }
        }
        self.write_custom(&props)
    }

    /// Removes a custom property; returns whether it existed.
    pub fn remove_custom_property(&mut self, name: &str) -> Result<bool> {
        let mut props = self.read_custom()?;
        let before = props.property.len();
        props.property.retain(|p| p.name.as_deref() != Some(name));
        if props.property.len() == before {
            return Ok(false);
        }
        self.write_custom(&props)?;
        Ok(true)
    }

    // ----- application properties -------------------------------------------------------

    fn app_part(&self) -> Option<PartName> {
        self.shared
            .package
            .related_part(None, rel_types::EXTENDED_PROPERTIES)
            .filter(|p| self.shared.package.contains(p))
    }

    /// Application properties (`docProps/app.xml`): application, company,
    /// statistics, … (default values when the part is missing).
    pub fn app_properties(&self) -> Result<ep::CT_Properties> {
        match self.app_part() {
            Some(name) => read_part(&self.shared.package, &name, &ep::elements::PROPERTIES),
            None => Ok(ep::CT_Properties::default()),
        }
    }

    /// Replaces the application properties (creating the part when missing).
    pub fn set_app_properties(&mut self, props: &ep::CT_Properties) -> Result<()> {
        let name = match self.app_part() {
            Some(name) => name,
            None => {
                let name = PartName::new("/docProps/app.xml")?;
                self.shared
                    .package
                    .add_part(name.clone(), ct::EXTENDED_PROPERTIES, Vec::new())?;
                self.shared
                    .package
                    .add_relationship(None, rel_types::EXTENDED_PROPERTIES, &name)?;
                name
            }
        };
        write_part(
            &mut self.shared.package,
            &name,
            ct::EXTENDED_PROPERTIES,
            &ep::elements::PROPERTIES,
            props,
        )
    }

    /// Updates the word, character and paragraph counts of the application
    /// properties from the body text (page and line counts need a layout
    /// engine and are left unchanged).
    pub fn update_statistics(&mut self) -> Result<()> {
        let text = self.text();
        let mut props = self.app_properties()?;
        let clamp = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        props.words = Some(clamp(text.split_whitespace().count()));
        props.characters = Some(clamp(text.chars().filter(|c| !c.is_whitespace()).count()));
        props.characters_with_spaces = Some(clamp(text.chars().filter(|c| *c != '\n').count()));
        props.paragraphs = Some(clamp(
            crate::text::blocks(&self.body().block_level_elts)
                .iter()
                .filter(|b| matches!(b, crate::text::BlockRef::P(p) if !crate::text::paragraph_text(p).is_empty()))
                .count(),
        ));
        self.set_app_properties(&props)
    }

    // ----- settings ---------------------------------------------------------------------

    /// Sets the zoom percentage of the document window.
    pub fn set_zoom(&mut self, percent: u32) -> Result<()> {
        if !(10..=500).contains(&percent) {
            return Err(Error::InvalidArgument(format!(
                "zoom {percent}% is not in 10..=500"
            )));
        }
        self.shared.settings_mut()?.zoom = Some(Box::new(wml::CT_Zoom {
            percent: Some(wml::ST_DecimalNumberOrPercent::UnqualifiedPercentage(i64::from(
                percent,
            ))),
            ..Default::default()
        }));
        Ok(())
    }

    /// Zoom percentage, when set.
    pub fn zoom(&self) -> Option<u32> {
        match self.settings()?.zoom.as_deref()?.percent.as_ref()? {
            wml::ST_DecimalNumberOrPercent::UnqualifiedPercentage(v) => u32::try_from(*v).ok(),
            wml::ST_DecimalNumberOrPercent::Percentage(s) => {
                s.trim_end_matches('%').parse::<f64>().ok().map(|v| v as u32)
            }
        }
    }

    /// Sets the interval of the default tab stops.
    pub fn set_default_tab_stop(&mut self, interval: Length) -> Result<()> {
        self.shared.settings_mut()?.default_tab_stop = Some(Box::new(wml::CT_TwipsMeasure {
            val: Some(twips(interval)),
            ..Default::default()
        }));
        Ok(())
    }

    /// Interval of the default tab stops, when set.
    pub fn default_tab_stop(&self) -> Option<Length> {
        self.settings()?
            .default_tab_stop
            .as_deref()?
            .val
            .as_ref()
            .and_then(twips_value)
    }

    /// Sets Word's compatibility mode (`15` for Word 2013 and later).
    pub fn set_compatibility_mode(&mut self, mode: u32) -> Result<()> {
        let compat = self
            .shared
            .settings_mut()?
            .compat
            .get_or_insert_with(Default::default);
        compat
            .compat_setting
            .retain(|c| c.name.as_deref() != Some("compatibilityMode"));
        compat.compat_setting.insert(
            0,
            wml::CT_CompatSetting {
                name: Some("compatibilityMode".into()),
                uri: Some(COMPAT_URI.into()),
                val: Some(mode.to_string()),
                ..Default::default()
            },
        );
        Ok(())
    }

    /// Word's compatibility mode, when set.
    pub fn compatibility_mode(&self) -> Option<u32> {
        self.settings()?
            .compat
            .as_deref()?
            .compat_setting
            .iter()
            .find(|c| c.name.as_deref() == Some("compatibilityMode"))?
            .val
            .as_deref()?
            .parse()
            .ok()
    }

    // ----- protection -------------------------------------------------------------------

    /// Restricts editing (`w:documentProtection` with enforcement). With a
    /// password, the legacy password hash of ECMA-376 Part 4 §14.8.1 is
    /// stored, salted and iterated with SHA-1 (`cryptAlgorithmSid` 4).
    ///
    /// The hash follows the standard literally (the four key bytes are
    /// hashed). Microsoft documents a different key encoding for Word in
    /// MS-OI29500, so Word may not accept the password to lift the
    /// restriction; the restriction itself is honored either way.
    ///
    /// ```
    /// use openxml_docx::{Document, Protection};
    ///
    /// let mut doc = Document::new();
    /// doc.protect(Protection::ReadOnly, Some("secret"))?;
    /// assert_eq!(doc.protection(), Some(Protection::ReadOnly));
    /// assert!(doc.check_protection_password("secret"));
    /// assert!(!doc.check_protection_password("guess"));
    /// # Ok::<(), openxml_docx::Error>(())
    /// ```
    pub fn protect(&mut self, kind: Protection, password: Option<&str>) -> Result<()> {
        let mut prot = wml::CT_DocProtect {
            edit: Some(match kind {
                Protection::ReadOnly => ST_DocProtect::ReadOnly,
                Protection::Comments => ST_DocProtect::Comments,
                Protection::TrackedChanges => ST_DocProtect::TrackedChanges,
                Protection::Forms => ST_DocProtect::Forms,
            }),
            enforcement: Some(ST_OnOff::Boolean(true)),
            ..Default::default()
        };
        if let Some(password) = password {
            let salt = random_salt();
            let hash = protection_hash(password, &salt, SPIN_COUNT);
            prot.crypt_provider_type = Some(ST_CryptProv::RsaFull);
            prot.crypt_algorithm_class = Some(ST_AlgClass::Hash);
            prot.crypt_algorithm_type = Some(ST_AlgType::TypeAny);
            prot.crypt_algorithm_sid = Some(4);
            prot.crypt_spin_count = Some(i64::from(SPIN_COUNT));
            prot.hash = Some(Base64Binary(hash.to_vec()));
            prot.salt = Some(Base64Binary(salt.to_vec()));
        }
        self.shared.settings_mut()?.document_protection = Some(Box::new(prot));
        Ok(())
    }

    /// Removes the editing restriction.
    pub fn unprotect(&mut self) -> Result<()> {
        if self.settings().is_some_and(|s| s.document_protection.is_some()) {
            self.shared.settings_mut()?.document_protection = None;
        }
        Ok(())
    }

    /// The enforced editing restriction, if any.
    pub fn protection(&self) -> Option<Protection> {
        let p = self.settings()?.document_protection.as_deref()?;
        let enforced = matches!(
            p.enforcement,
            Some(ST_OnOff::Boolean(true) | ST_OnOff::OnOff1(openxml_schema::shared_types::ST_OnOff1::On))
        );
        if !enforced {
            return None;
        }
        Some(match p.edit? {
            ST_DocProtect::ReadOnly => Protection::ReadOnly,
            ST_DocProtect::Comments => Protection::Comments,
            ST_DocProtect::TrackedChanges => Protection::TrackedChanges,
            ST_DocProtect::Forms => Protection::Forms,
            ST_DocProtect::None => return None,
        })
    }

    /// Whether `password` matches the stored protection hash (SHA-1 legacy
    /// hashes as written by [`Document::protect`]). A protection without a
    /// hash accepts any password.
    pub fn check_protection_password(&self, password: &str) -> bool {
        let Some(p) = self.settings().and_then(|s| s.document_protection.as_deref()) else {
            return true;
        };
        let (Some(hash), Some(salt)) = (&p.hash, &p.salt) else {
            return true;
        };
        if p.crypt_algorithm_sid.unwrap_or(4) != 4 {
            return false;
        }
        let spin = p
            .crypt_spin_count
            .and_then(|s| u32::try_from(s).ok())
            .unwrap_or(0);
        protection_hash(password, &salt.0, spin).as_slice() == hash.0.as_slice()
    }

    // ----- background -------------------------------------------------------------------

    /// Sets the page background color (`w:background`) and turns on its
    /// display (`w:displayBackgroundShape`).
    pub fn set_background_color(&mut self, hex: &str) -> Result<()> {
        let color = hex_color(hex)?;
        self.document_mut().background = Some(Box::new(wml::CT_Background {
            color: Some(color),
            ..Default::default()
        }));
        self.shared.settings_mut()?.display_background_shape = Some(on());
        Ok(())
    }

    /// The page background color, when set.
    pub fn background_color(&self) -> Option<String> {
        self.main
            .background
            .as_deref()?
            .color
            .as_ref()
            .map(hex_color_string)
    }

    /// Whether the background is displayed.
    pub fn displays_background(&self) -> bool {
        self.settings()
            .is_some_and(|s| is_on(&s.display_background_shape))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha1_test_vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hex(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            hex(&sha1(&vec![b'a'; 1_000_000])),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    #[test]
    fn legacy_key_matches_the_spec_example() {
        // ECMA-376 Part 4 §14.8.1: "Example" gives 0x64CEED7E.
        assert_eq!(legacy_key("Example"), 0x64CE_ED7E);
        assert_eq!(legacy_key(""), 0);
        // Only the first 15 characters count.
        assert_eq!(legacy_key("0123456789abcdefXYZ"), legacy_key("0123456789abcde"));
        // Characters whose low byte is zero use their high byte.
        assert_eq!(legacy_key("\u{4100}"), legacy_key("A"));
    }

    #[test]
    fn protection_hash_iterates() {
        let salt = [1u8; 16];
        let zero = protection_hash("Example", &salt, 0);
        let mut input = salt.to_vec();
        input.extend_from_slice(&[0x7E, 0xED, 0xCE, 0x64]);
        assert_eq!(zero, sha1(&input));
        let one = protection_hash("Example", &salt, 1);
        let mut next = zero.to_vec();
        next.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(one, sha1(&next));
        assert_ne!(random_salt(), [0u8; 16]);
    }

    #[test]
    fn property_values_map_to_variant_types() {
        for v in [
            PropertyValue::Text("x".into()),
            PropertyValue::Integer(42),
            PropertyValue::Integer(1 << 40),
            PropertyValue::Number(2.5),
            PropertyValue::Bool(true),
            PropertyValue::Date("2024-01-01T00:00:00Z".into()),
        ] {
            assert_eq!(PropertyValue::from_choice(&v.to_choice()), Some(v));
        }
        assert!(matches!(
            PropertyValue::Integer(1).to_choice(),
            cp::CT_Property_Choice::I4(1)
        ));
        assert!(matches!(
            PropertyValue::Integer(1 << 40).to_choice(),
            cp::CT_Property_Choice::I8(_)
        ));
        assert_eq!(
            PropertyValue::from_choice(&cp::CT_Property_Choice::Ui2(7)),
            Some(PropertyValue::Integer(7))
        );
        assert_eq!(
            PropertyValue::from_choice(&cp::CT_Property_Choice::Blob(Base64Binary(vec![]))),
            None
        );
    }
}
