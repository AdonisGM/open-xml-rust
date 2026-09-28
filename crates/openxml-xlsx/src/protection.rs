//! Sheet and workbook protection with password hashes (ECMA-376 Part 1
//! §18.3.1.85, §18.2.29).
//!
//! Protection is an editing restriction honoured by Excel, not encryption:
//! the content stays readable. Two password hashes exist: the legacy 16-bit
//! hash (understood by every version) and the salted, iterated SHA-512 hash
//! Excel 2013+ writes.
//!
//! ```
//! use openxml_xlsx::{Workbook, SheetProtection, CellStyle, WorkbookProtection};
//!
//! let mut wb = Workbook::new();
//! let editable = wb.add_style(&CellStyle::new().unlocked());
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.set_cell_style("B2", editable)?;
//! sheet.protect(&SheetProtection::new().password("secret").allow_sort())?;
//! assert!(sheet.as_view().verify_sheet_password("secret"));
//! wb.protect_workbook(&WorkbookProtection::new().password("book").sha512(1_000));
//! assert!(wb.verify_workbook_password("book"));
//! # Ok::<(), openxml_core::Error>(())
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use openxml_core::{Error, Result};
use openxml_schema::sml;
use openxml_xml::{Base64Binary, HexBinary};

use crate::workbook::Workbook;
use crate::worksheet::{Worksheet, WorksheetMut};

// ----- SHA-512 (FIPS 180-4) --------------------------------------------------

const K: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

const H0: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

fn compress(h: &mut [u64; 8], block: &[u8]) {
    let mut w = [0u64; 80];
    for (i, word) in w.iter_mut().take(16).enumerate() {
        *word = u64::from_be_bytes(block[i * 8..i * 8 + 8].try_into().expect("8 bytes"));
    }
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for i in 0..80 {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (x, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
        *x = x.wrapping_add(v);
    }
}

/// SHA-512 digest of `data`.
pub fn sha512(data: &[u8]) -> [u8; 64] {
    let mut h = H0;
    let (blocks, rest) = data.as_chunks::<128>();
    for block in blocks {
        compress(&mut h, block);
    }
    let mut tail = Vec::with_capacity(256);
    tail.extend_from_slice(rest);
    tail.push(0x80);
    while tail.len() % 128 != 112 {
        tail.push(0);
    }
    tail.extend_from_slice(&((data.len() as u128) * 8).to_be_bytes());
    for block in tail.as_chunks::<128>().0 {
        compress(&mut h, block);
    }
    let mut out = [0u8; 64];
    for (i, v) in h.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&v.to_be_bytes());
    }
    out
}

// ----- password hashes -------------------------------------------------------

/// The legacy 16-bit password hash of §18.2.29 (written as `password="83AF"`).
///
/// Characters are taken by their UTF-16 code unit; Excel uses the low byte
/// of the ANSI code page, so non-ASCII passwords may hash differently.
pub fn legacy_password_hash(password: &str) -> u16 {
    let units: Vec<u16> = password.encode_utf16().collect();
    let rotate = |h: u16| ((h >> 14) & 0x01) | ((h << 1) & 0x7fff);
    let mut hash: u16 = 0;
    for &c in units.iter().rev() {
        hash = rotate(hash) ^ c;
    }
    hash = rotate(hash);
    hash ^= units.len() as u16;
    hash ^ 0xCE4B
}

/// The iterated SHA-512 password hash of ECMA-376 Part 1 §18.2.29:
/// `H₀ = SHA-512(salt ‖ UTF-16LE(password))`, then `spin_count` rounds of
/// `Hᵢ₊₁ = SHA-512(Hᵢ ‖ i as 32-bit little endian)`.
pub fn sha512_password_hash(password: &str, salt: &[u8], spin_count: u32) -> [u8; 64] {
    let mut input = salt.to_vec();
    for u in password.encode_utf16() {
        input.extend_from_slice(&u.to_le_bytes());
    }
    let mut h = sha512(&input);
    let mut buf = [0u8; 68];
    for i in 0..spin_count {
        buf[..64].copy_from_slice(&h);
        buf[64..].copy_from_slice(&i.to_le_bytes());
        h = sha512(&buf);
    }
    h
}

/// A fresh 16-byte salt (from the clock, a counter and addresses; salts
/// need to be unique, not secret).
fn new_salt() -> Vec<u8> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let local = 0u8;
    let mut seed = Vec::new();
    seed.extend_from_slice(&nanos.to_le_bytes());
    seed.extend_from_slice(&n.to_le_bytes());
    seed.extend_from_slice(&(std::ptr::addr_of!(local) as usize).to_le_bytes());
    seed.extend_from_slice(&std::process::id().to_le_bytes());
    sha512(&seed)[..16].to_vec()
}

/// How a protection password is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasswordHash {
    /// The legacy 16-bit hash.
    Legacy,
    /// Salted SHA-512 with `spin_count` iterations (Excel uses 100 000).
    Sha512 {
        /// Number of iterations.
        spin_count: u32,
        /// Salt; a random one is generated when `None`.
        salt: Option<Vec<u8>>,
    },
}

/// A hashed password: (legacy hash) or (algorithm, hash, salt, spin count).
enum Hashed {
    Legacy(u16),
    Modern(Vec<u8>, Vec<u8>, u32),
}

fn hash(password: &str, how: &PasswordHash) -> Hashed {
    match how {
        PasswordHash::Legacy => Hashed::Legacy(legacy_password_hash(password)),
        PasswordHash::Sha512 { spin_count, salt } => {
            let salt = salt.clone().unwrap_or_else(new_salt);
            let h = sha512_password_hash(password, &salt, *spin_count);
            Hashed::Modern(h.to_vec(), salt, *spin_count)
        }
    }
}

fn verify(
    password: &str,
    legacy: Option<&HexBinary>,
    algorithm: Option<&str>,
    hash_value: Option<&Base64Binary>,
    salt: Option<&Base64Binary>,
    spin: Option<u32>,
) -> bool {
    if let (Some(alg), Some(h)) = (algorithm, hash_value) {
        if !alg.eq_ignore_ascii_case("SHA-512") {
            return false;
        }
        let salt = salt.map(|s| s.0.as_slice()).unwrap_or(&[]);
        return sha512_password_hash(password, salt, spin.unwrap_or(0)).as_slice() == h.0.as_slice();
    }
    match legacy {
        Some(h) => h.0.as_slice() == legacy_password_hash(password).to_be_bytes(),
        None => password.is_empty(),
    }
}

// ----- sheet protection ------------------------------------------------------

/// Sheet protection: an optional password and the actions users may still
/// perform. Cells stay editable only if their style is unlocked (see
/// [`crate::CellStyle::unlocked`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetProtection {
    password: Option<String>,
    hash: PasswordHash,
    /// Select locked cells (default allowed).
    pub select_locked_cells: bool,
    /// Select unlocked cells (default allowed).
    pub select_unlocked_cells: bool,
    /// Format cells.
    pub format_cells: bool,
    /// Format columns.
    pub format_columns: bool,
    /// Format rows.
    pub format_rows: bool,
    /// Insert columns.
    pub insert_columns: bool,
    /// Insert rows.
    pub insert_rows: bool,
    /// Insert hyperlinks.
    pub insert_hyperlinks: bool,
    /// Delete columns.
    pub delete_columns: bool,
    /// Delete rows.
    pub delete_rows: bool,
    /// Sort.
    pub sort: bool,
    /// Use the auto filter.
    pub auto_filter: bool,
    /// Use pivot tables.
    pub pivot_tables: bool,
    /// Edit drawing objects.
    pub edit_objects: bool,
    /// Edit scenarios.
    pub edit_scenarios: bool,
}

impl Default for SheetProtection {
    fn default() -> Self {
        Self::new()
    }
}

impl SheetProtection {
    /// Protection without a password, with Excel's default permissions
    /// (only cell selection allowed).
    pub fn new() -> Self {
        SheetProtection {
            password: None,
            hash: PasswordHash::Legacy,
            select_locked_cells: true,
            select_unlocked_cells: true,
            format_cells: false,
            format_columns: false,
            format_rows: false,
            insert_columns: false,
            insert_rows: false,
            insert_hyperlinks: false,
            delete_columns: false,
            delete_rows: false,
            sort: false,
            auto_filter: false,
            pivot_tables: false,
            edit_objects: false,
            edit_scenarios: false,
        }
    }

    /// Requires a password to unprotect (stored as the legacy hash unless
    /// [`SheetProtection::sha512`] is used).
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into()).filter(|p: &String| !p.is_empty());
        self
    }

    /// Stores the password as a salted SHA-512 hash with `spin_count` iterations.
    pub fn sha512(mut self, spin_count: u32) -> Self {
        self.hash = PasswordHash::Sha512 {
            spin_count,
            salt: None,
        };
        self
    }

    /// Sets how the password is hashed.
    pub fn hash(mut self, how: PasswordHash) -> Self {
        self.hash = how;
        self
    }

    /// Allows formatting cells.
    pub fn allow_format_cells(mut self) -> Self {
        self.format_cells = true;
        self
    }

    /// Allows sorting.
    pub fn allow_sort(mut self) -> Self {
        self.sort = true;
        self
    }

    /// Allows using the auto filter.
    pub fn allow_auto_filter(mut self) -> Self {
        self.auto_filter = true;
        self
    }

    /// Allows inserting and deleting rows.
    pub fn allow_row_edits(mut self) -> Self {
        self.insert_rows = true;
        self.delete_rows = true;
        self
    }

    fn to_ct(&self) -> sml::CT_SheetProtection {
        // Attributes say what is *protected*; defaults differ per attribute.
        let locked = |allowed: bool| (allowed).then_some(false);
        let mut p = sml::CT_SheetProtection {
            sheet: Some(true),
            objects: (!self.edit_objects).then_some(true),
            scenarios: (!self.edit_scenarios).then_some(true),
            format_cells: locked(self.format_cells),
            format_columns: locked(self.format_columns),
            format_rows: locked(self.format_rows),
            insert_columns: locked(self.insert_columns),
            insert_rows: locked(self.insert_rows),
            insert_hyperlinks: locked(self.insert_hyperlinks),
            delete_columns: locked(self.delete_columns),
            delete_rows: locked(self.delete_rows),
            select_locked_cells: (!self.select_locked_cells).then_some(true),
            sort: locked(self.sort),
            auto_filter: locked(self.auto_filter),
            pivot_tables: locked(self.pivot_tables),
            select_unlocked_cells: (!self.select_unlocked_cells).then_some(true),
            ..Default::default()
        };
        if let Some(pw) = &self.password {
            match hash(pw, &self.hash) {
                Hashed::Legacy(h) => p.password = Some(HexBinary(h.to_be_bytes().to_vec())),
                Hashed::Modern(h, salt, spin) => {
                    p.algorithm_name = Some("SHA-512".into());
                    p.hash_value = Some(Base64Binary(h));
                    p.salt_value = Some(Base64Binary(salt));
                    p.spin_count = Some(spin);
                }
            }
        }
        p
    }

    fn from_ct(p: &sml::CT_SheetProtection) -> Self {
        let allowed = |v: Option<bool>| v == Some(false);
        SheetProtection {
            password: None,
            hash: match (&p.algorithm_name, p.spin_count) {
                (Some(_), Some(spin_count)) => PasswordHash::Sha512 {
                    spin_count,
                    salt: p.salt_value.as_ref().map(|s| s.0.clone()),
                },
                _ => PasswordHash::Legacy,
            },
            select_locked_cells: p.select_locked_cells != Some(true),
            select_unlocked_cells: p.select_unlocked_cells != Some(true),
            format_cells: allowed(p.format_cells),
            format_columns: allowed(p.format_columns),
            format_rows: allowed(p.format_rows),
            insert_columns: allowed(p.insert_columns),
            insert_rows: allowed(p.insert_rows),
            insert_hyperlinks: allowed(p.insert_hyperlinks),
            delete_columns: allowed(p.delete_columns),
            delete_rows: allowed(p.delete_rows),
            sort: allowed(p.sort),
            auto_filter: allowed(p.auto_filter),
            pivot_tables: allowed(p.pivot_tables),
            edit_objects: p.objects != Some(true),
            edit_scenarios: p.scenarios != Some(true),
        }
    }
}

impl Worksheet<'_> {
    /// The sheet's protection settings (the password itself cannot be
    /// recovered; see [`Worksheet::sheet_has_password`]).
    pub fn protection(&self) -> Option<SheetProtection> {
        self.data
            .sheet_protection
            .as_deref()
            .filter(|p| p.sheet == Some(true))
            .map(SheetProtection::from_ct)
    }

    /// Whether unprotecting the sheet requires a password.
    pub fn sheet_has_password(&self) -> bool {
        self.data
            .sheet_protection
            .as_deref()
            .is_some_and(|p| p.password.is_some() || p.hash_value.is_some())
    }

    /// Whether `password` unprotects the sheet.
    pub fn verify_sheet_password(&self, password: &str) -> bool {
        let Some(p) = self.data.sheet_protection.as_deref() else {
            return false;
        };
        verify(
            password,
            p.password.as_ref(),
            p.algorithm_name.as_deref(),
            p.hash_value.as_ref(),
            p.salt_value.as_ref(),
            p.spin_count,
        )
    }
}

impl WorksheetMut<'_> {
    /// Protects the sheet.
    pub fn protect(&mut self, protection: &SheetProtection) -> Result<()> {
        if let PasswordHash::Sha512 { spin_count, .. } = protection.hash
            && spin_count > 10_000_000
        {
            return Err(Error::InvalidArgument("spin count above 10 000 000".into()));
        }
        self.data.sheet_protection = Some(Box::new(protection.to_ct()));
        Ok(())
    }

    /// Removes the sheet protection. Returns whether the sheet was protected.
    pub fn unprotect(&mut self) -> bool {
        self.data.sheet_protection.take().is_some()
    }
}

// ----- workbook protection ---------------------------------------------------

/// Workbook protection: locks the sheet structure and/or the window layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkbookProtection {
    password: Option<String>,
    hash: PasswordHash,
    /// Sheets cannot be added, moved, renamed, hidden or deleted.
    pub lock_structure: bool,
    /// Workbook windows cannot be moved or resized.
    pub lock_windows: bool,
}

impl Default for WorkbookProtection {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkbookProtection {
    /// Structure protection without a password.
    pub fn new() -> Self {
        WorkbookProtection {
            password: None,
            hash: PasswordHash::Legacy,
            lock_structure: true,
            lock_windows: false,
        }
    }

    /// Requires a password to unprotect.
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into()).filter(|p: &String| !p.is_empty());
        self
    }

    /// Stores the password as a salted SHA-512 hash.
    pub fn sha512(mut self, spin_count: u32) -> Self {
        self.hash = PasswordHash::Sha512 {
            spin_count,
            salt: None,
        };
        self
    }

    /// Sets how the password is hashed.
    pub fn hash(mut self, how: PasswordHash) -> Self {
        self.hash = how;
        self
    }

    /// Also locks the windows.
    pub fn lock_windows(mut self, lock: bool) -> Self {
        self.lock_windows = lock;
        self
    }
}

impl Workbook {
    /// Protects the workbook structure (and optionally its windows).
    pub fn protect_workbook(&mut self, protection: &WorkbookProtection) {
        let mut p = sml::CT_WorkbookProtection {
            lock_structure: protection.lock_structure.then_some(true),
            lock_windows: protection.lock_windows.then_some(true),
            ..Default::default()
        };
        if let Some(pw) = &protection.password {
            match hash(pw, &protection.hash) {
                Hashed::Legacy(h) => p.workbook_password = Some(HexBinary(h.to_be_bytes().to_vec())),
                Hashed::Modern(h, salt, spin) => {
                    p.workbook_algorithm_name = Some("SHA-512".into());
                    p.workbook_hash_value = Some(Base64Binary(h));
                    p.workbook_salt_value = Some(Base64Binary(salt));
                    p.workbook_spin_count = Some(spin);
                }
            }
        }
        self.raw_workbook_mut().workbook_protection = Some(Box::new(p));
    }

    /// Removes workbook protection. Returns whether it was protected.
    pub fn unprotect_workbook(&mut self) -> bool {
        let was = self.workbook.workbook_protection.is_some();
        if was {
            self.raw_workbook_mut().workbook_protection = None;
        }
        was
    }

    /// Whether the structure and the windows are locked: `(structure, windows)`.
    pub fn workbook_protection(&self) -> Option<(bool, bool)> {
        let p = self.workbook.workbook_protection.as_deref()?;
        Some((p.lock_structure.unwrap_or(false), p.lock_windows.unwrap_or(false)))
    }

    /// Whether `password` unprotects the workbook.
    pub fn verify_workbook_password(&self, password: &str) -> bool {
        let Some(p) = self.workbook.workbook_protection.as_deref() else {
            return false;
        };
        verify(
            password,
            p.workbook_password.as_ref(),
            p.workbook_algorithm_name.as_deref(),
            p.workbook_hash_value.as_ref(),
            p.workbook_salt_value.as_ref(),
            p.workbook_spin_count,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openxml_xml::XmlValue;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha512_test_vectors() {
        // FIPS 180-4 examples.
        assert_eq!(
            hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            hex(&sha512(b"")),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
        assert_eq!(
            hex(&sha512(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")),
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        );
        // Exactly one block of padding boundary (112 bytes) and a million 'a'.
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&sha512(&million)),
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        );
    }

    #[test]
    fn legacy_hash_vectors() {
        assert_eq!(legacy_password_hash("password"), 0x83AF);
        assert_eq!(legacy_password_hash("secret"), 0xDAA7);
        assert_eq!(legacy_password_hash("test"), 0xCBEB);
        assert_eq!(legacy_password_hash(""), 0xCE4B);
    }

    #[test]
    fn excel_2013_sha512_vector() {
        // From workbookProtection-sheet_password-2013.xlsx (password "pwd").
        let salt = Base64Binary::parse_xml("R040EdN/Ec7il6MJ8JrRLQ==").unwrap();
        let expected = Base64Binary::parse_xml(
            "5MANCkOK6IY02H1LhiJ+ucR5ZHvoV7BwbINSx52iIhe4Xfg986k2l32ONsYpt8JPiy8U8kqPRKXIr7G8hfMWOw==",
        )
        .unwrap();
        assert_eq!(
            sha512_password_hash("pwd", &salt.0, 100_000).as_slice(),
            expected.0.as_slice()
        );
        assert!(verify(
            "pwd",
            None,
            Some("SHA-512"),
            Some(&expected),
            Some(&salt),
            Some(100_000)
        ));
        assert!(!verify(
            "pwe",
            None,
            Some("SHA-512"),
            Some(&expected),
            Some(&salt),
            Some(100_000)
        ));
    }

    #[test]
    fn protection_attributes() {
        let p = SheetProtection::new().to_ct();
        assert_eq!(p.sheet, Some(true));
        assert_eq!((p.objects, p.scenarios), (Some(true), Some(true)));
        assert_eq!(p.format_cells, None, "protected by default");
        assert_eq!(p.select_locked_cells, None, "allowed by default");
        let mut all = SheetProtection::new()
            .allow_format_cells()
            .allow_sort()
            .allow_auto_filter()
            .allow_row_edits();
        all.select_locked_cells = false;
        all.edit_objects = true;
        let p = all.to_ct();
        assert_eq!(
            (p.format_cells, p.sort, p.auto_filter),
            (Some(false), Some(false), Some(false))
        );
        assert_eq!((p.insert_rows, p.delete_rows), (Some(false), Some(false)));
        assert_eq!(p.select_locked_cells, Some(true));
        assert_eq!(p.objects, None);
        assert_eq!(SheetProtection::from_ct(&p), all);
        let salt = vec![7u8; 16];
        let hashed = SheetProtection::new()
            .password("abc")
            .hash(PasswordHash::Sha512 {
                spin_count: 10,
                salt: Some(salt.clone()),
            })
            .to_ct();
        assert_eq!(hashed.salt_value.as_ref().unwrap().0, salt);
        assert_eq!(
            hashed.hash_value.as_ref().unwrap().0,
            sha512_password_hash("abc", &salt, 10)
        );
        assert_ne!(new_salt(), new_salt(), "salts are unique");
    }
}
