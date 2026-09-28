//! Access to the ECMA-376 Part 1 section index (`schemas/spec-index.json`).

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

/// A row of an attribute or enumeration table.
#[derive(Debug, Clone, Deserialize)]
pub struct Row {
    /// Attribute name or enumeration value.
    pub name: String,
    /// Human-readable title.
    pub title: String,
    /// First sentence of the description.
    pub description: String,
}

/// A reference section for an element or simple type.
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    /// Section number, e.g. `17.3.1.22`.
    pub section: String,
    /// Namespace prefix, e.g. `w`.
    pub ns: String,
    /// Element or type name.
    pub name: String,
    /// Section title, e.g. `Paragraph`.
    pub title: String,
    /// PDF page number.
    pub page: u32,
    /// First descriptive paragraph.
    pub description: String,
    /// Schema type (`CT_…` / `ST_…`).
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    /// `element` or `simpleType`.
    pub kind: String,
    /// Attribute table (elements).
    #[serde(default)]
    pub attributes: Vec<Row>,
    /// Enumeration table (simple types).
    #[serde(default)]
    pub values: Vec<Row>,
}

#[derive(Deserialize)]
struct File {
    entries: Vec<Entry>,
}

/// Lookup structure over the index.
#[derive(Default)]
pub struct SpecIndex {
    entries: Vec<Entry>,
    by_type: HashMap<(String, String), Vec<usize>>,
    by_name: HashMap<(String, String), Vec<usize>>,
}

impl SpecIndex {
    /// An index without entries (documentation falls back to schema names).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Loads the index from JSON.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text)
    }

    /// Parses the index from JSON text.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut idx = SpecIndex::default();
        for (i, e) in file.entries.iter().enumerate() {
            if let Some(t) = &e.type_name {
                idx.by_type.entry((e.ns.clone(), t.clone())).or_default().push(i);
            }
            idx.by_name
                .entry((e.ns.clone(), e.name.clone()))
                .or_default()
                .push(i);
        }
        idx.entries = file.entries;
        Ok(idx)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Element sections whose content model is `type_name`.
    pub fn elements_of_type(&self, ns: &str, type_name: &str) -> Vec<&Entry> {
        self.by_type
            .get(&(ns.to_owned(), type_name.to_owned()))
            .map(|v| {
                v.iter()
                    .map(|&i| &self.entries[i])
                    .filter(|e| e.kind == "element")
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The section of element `name`, preferring the one with type `type_name`.
    pub fn element(&self, ns: &str, name: &str, type_name: Option<&str>) -> Option<&Entry> {
        let candidates: Vec<&Entry> = self
            .by_name
            .get(&(ns.to_owned(), name.to_owned()))
            .map(|v| {
                v.iter()
                    .map(|&i| &self.entries[i])
                    .filter(|e| e.kind == "element")
                    .collect()
            })
            .unwrap_or_default();
        if let Some(t) = type_name
            && let Some(e) = candidates.iter().find(|e| e.type_name.as_deref() == Some(t))
        {
            return Some(e);
        }
        (candidates.len() == 1).then(|| candidates[0])
    }

    /// The section of simple type `name`.
    pub fn simple_type(&self, ns: &str, name: &str) -> Option<&Entry> {
        self.by_name.get(&(ns.to_owned(), name.to_owned())).and_then(|v| {
            v.iter()
                .map(|&i| &self.entries[i])
                .find(|e| e.kind == "simpleType")
        })
    }
}

/// Maps a namespace identifier (as in `openxml_xml::Ns`) to the prefix used in the index.
pub fn spec_prefix(ns_ident: &str) -> Option<&'static str> {
    Some(match ns_ident {
        "W" => "w",
        "X" => "x",
        "P" => "p",
        "A" => "a",
        "PIC" => "pic",
        "LC" => "lc",
        "WP" => "wp",
        "XDR" => "xdr",
        "C" => "c",
        "CDR" => "cdr",
        "DGM" => "dgm",
        "M" => "m",
        "EP" => "ep",
        "OP" => "op",
        "VT" => "vt",
        "DS" => "ds",
        "B" => "b",
        "AC" => "ac",
        "R" => "r",
        "S" => "s",
        "SL" => "sl",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"entries":[
      {"section":"17.3.1.22","ns":"w","name":"p","title":"Paragraph","page":245,"description":"A paragraph.","type":"CT_P","kind":"element","attributes":[{"name":"rsidR","title":"Revision","description":"Id."}]},
      {"section":"17.3.1.25","ns":"w","name":"pPr","title":"Previous Paragraph Properties","page":248,"description":"Old.","type":"CT_PPrBase","kind":"element","attributes":[]},
      {"section":"17.3.1.26","ns":"w","name":"pPr","title":"Paragraph Properties","page":249,"description":"New.","type":"CT_PPr","kind":"element","attributes":[]},
      {"section":"17.18.44","ns":"w","name":"ST_Jc","title":"Horizontal Alignment Type","page":1407,"description":"Alignment.","type":"ST_Jc","kind":"simpleType","values":[{"name":"both","title":"Justified","description":"x"}]}
    ]}"#;

    #[test]
    fn lookups() {
        let idx = SpecIndex::from_json(JSON).unwrap();
        assert_eq!(idx.len(), 4);
        assert!(!idx.is_empty());
        assert_eq!(idx.elements_of_type("w", "CT_P")[0].section, "17.3.1.22");
        assert_eq!(
            idx.element("w", "pPr", Some("CT_PPr")).unwrap().section,
            "17.3.1.26"
        );
        assert_eq!(
            idx.element("w", "pPr", Some("CT_PPrBase")).unwrap().section,
            "17.3.1.25"
        );
        assert!(
            idx.element("w", "pPr", None).is_none(),
            "ambiguous without a type"
        );
        assert_eq!(idx.element("w", "p", None).unwrap().title, "Paragraph");
        assert_eq!(
            idx.simple_type("w", "ST_Jc").unwrap().values[0].title,
            "Justified"
        );
        assert!(idx.simple_type("x", "ST_Jc").is_none());
        assert!(SpecIndex::empty().is_empty());
        assert!(SpecIndex::from_json("{").is_err());
        assert_eq!(spec_prefix("W"), Some("w"));
        assert_eq!(spec_prefix("V"), None);
    }

    #[test]
    fn loads_vendored_index() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/spec-index.json");
        let idx = SpecIndex::load(&path).unwrap();
        assert!(idx.len() > 2500);
        assert_eq!(idx.element("w", "p", Some("CT_P")).unwrap().section, "17.3.1.22");
        assert!(SpecIndex::load(Path::new("/nonexistent.json")).is_err());
    }
}
