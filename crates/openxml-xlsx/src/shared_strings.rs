//! The shared string table (`xl/sharedStrings.xml`, ECMA-376 Part 1 §18.4).

use std::collections::HashMap;

use openxml_schema::sml;
use openxml_xml::{ExtraChild, Ns, RawAttribute};

use crate::value::{decode_xstring, encode_xstring};

/// Text of a `<t>` element that the typed model kept as raw XML (this
/// happens when it carries attributes other than `xml:space`).
fn raw_t_text(extras: &[ExtraChild]) -> Option<String> {
    extras
        .iter()
        .find(|e| e.element.name.is(Ns::X, "t"))
        .map(|e| e.element.text())
}

/// Plain text of a rich string: the `<t>` of the item or the concatenated
/// runs; phonetic runs are excluded. `_xHHHH_` escapes are decoded.
pub fn rst_text(rst: &sml::CT_Rst) -> String {
    let mut out = String::new();
    match (&rst.t, raw_t_text(&rst.extra_children)) {
        (Some(t), _) => out.push_str(t),
        (None, Some(t)) => out.push_str(&t),
        (None, None) => {}
    }
    for run in &rst.r {
        match (&run.t, raw_t_text(&run.extra_children)) {
            (Some(t), _) => out.push_str(t),
            (None, Some(t)) => out.push_str(&t),
            (None, None) => {}
        }
    }
    decode_xstring(&out).into_owned()
}

/// Builds a plain (unformatted) string item. The writer adds
/// `xml:space="preserve"` to `<t>` when the text has leading or trailing
/// whitespace.
pub fn rst_from_text(text: &str) -> sml::CT_Rst {
    sml::CT_Rst {
        t: Some(encode_xstring(text).into_owned()),
        ..Default::default()
    }
}

/// The shared string table of a workbook.
#[derive(Debug, Clone, Default)]
pub struct SharedStrings {
    items: Vec<sml::CT_Rst>,
    texts: Vec<String>,
    lookup: HashMap<String, u32>,
    base_count: u32,
    added_refs: u32,
    dirty: bool,
    root_attrs: Vec<RawAttribute>,
}

impl SharedStrings {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads a table from the typed part.
    pub fn from_sst(sst: sml::CT_Sst) -> Self {
        let mut table = SharedStrings {
            base_count: sst.count.unwrap_or(sst.si.len() as u32),
            root_attrs: sst.extra_attrs,
            ..Default::default()
        };
        for item in sst.si {
            let text = rst_text(&item);
            let plain = item.r.is_empty() && item.r_ph.is_empty() && item.phonetic_pr.is_none();
            let index = table.items.len() as u32;
            if plain {
                table.lookup.entry(text.clone()).or_insert(index);
            }
            table.texts.push(text);
            table.items.push(item);
        }
        table
    }

    /// Number of distinct items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Plain text of item `index`.
    pub fn get(&self, index: u32) -> Option<&str> {
        self.texts.get(index as usize).map(String::as_str)
    }

    /// The typed (possibly rich) item `index`.
    pub fn item(&self, index: u32) -> Option<&sml::CT_Rst> {
        self.items.get(index as usize)
    }

    /// Returns the index of `text`, adding a plain item if needed, and counts
    /// one more reference.
    pub fn intern(&mut self, text: &str) -> u32 {
        self.added_refs += 1;
        if let Some(&i) = self.lookup.get(text) {
            return i;
        }
        let index = self.items.len() as u32;
        self.items.push(rst_from_text(text));
        self.texts.push(text.to_owned());
        self.lookup.insert(text.to_owned(), index);
        self.dirty = true;
        index
    }

    /// Whether the table changed since it was loaded.
    pub fn is_dirty(&self) -> bool {
        self.dirty || self.added_refs > 0
    }

    /// The typed part for saving.
    pub fn to_sst(&self) -> sml::CT_Sst {
        sml::CT_Sst {
            count: Some(self.base_count + self.added_refs),
            unique_count: Some(self.items.len() as u32),
            si: self.items.clone(),
            extra_attrs: self.root_attrs.clone(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sst(xml: &str) -> SharedStrings {
        SharedStrings::from_sst(sml::elements::SST.parse(xml).unwrap())
    }

    const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

    #[test]
    fn reads_plain_rich_and_phonetic_items() {
        let t = sst(&format!(
            r#"<sst xmlns="{NS}" count="5" uniqueCount="4">
                <si><t>plain</t></si>
                <si><r><rPr><b/></rPr><t>bo</t></r><r><t xml:space="preserve">ld text</t></r></si>
                <si><t xml:space="preserve"> padded </t></si>
                <si><t>漢字</t><rPh sb="0" eb="2"><t>かんじ</t></rPh></si>
            </sst>"#
        ));
        assert_eq!(t.len(), 4);
        assert_eq!(t.get(0), Some("plain"));
        assert_eq!(t.get(1), Some("bold text"));
        assert_eq!(t.get(2), Some(" padded "));
        assert_eq!(t.get(3), Some("漢字"), "phonetic runs are not part of the text");
        assert_eq!(t.get(4), None);
        assert!(t.item(1).unwrap().r.len() == 2);
        assert!(!t.is_dirty());
    }

    #[test]
    fn interning_deduplicates_plain_strings_only() {
        let mut t = sst(&format!(
            r#"<sst xmlns="{NS}"><si><t>a</t></si><si><r><t>b</t></r></si></sst>"#
        ));
        assert_eq!(t.intern("a"), 0);
        assert_eq!(t.intern("b"), 2, "a rich item is never reused for plain text");
        assert_eq!(t.intern("b"), 2);
        assert_eq!(t.intern(" c "), 3);
        assert!(t.is_dirty());
        let out = t.to_sst();
        assert_eq!(out.unique_count, Some(4));
        assert_eq!(out.count, Some(2 + 4));
        let xml = sml::elements::SST.to_xml(&out);
        assert!(
            xml.contains(r#"<si><t xml:space="preserve"> c </t></si>"#),
            "{xml}"
        );
        let back = SharedStrings::from_sst(sml::elements::SST.parse(&xml).unwrap());
        assert_eq!(back.get(3), Some(" c "));
        assert_eq!(back.get(2), Some("b"));
    }

    #[test]
    fn escapes_control_characters() {
        let mut t = SharedStrings::new();
        assert!(t.is_empty());
        let i = t.intern("line\r\nbreak");
        let xml = sml::elements::SST.to_xml(&t.to_sst());
        assert!(xml.contains("line_x000D_\nbreak"), "{xml}");
        let back = SharedStrings::from_sst(sml::elements::SST.parse(&xml).unwrap());
        assert_eq!(back.get(i), Some("line\r\nbreak"));
    }

    #[test]
    fn empty_table_serializes() {
        let t = SharedStrings::new();
        let out = t.to_sst();
        assert_eq!((out.count, out.unique_count), (Some(0), Some(0)));
    }
}
