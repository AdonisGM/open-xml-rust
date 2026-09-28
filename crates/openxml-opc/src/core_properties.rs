//! Core file properties (`docProps/core.xml`, ECMA-376 Part 2 §8.3).

use openxml_xml::{Ns, RawElement, XmlWriter};

use crate::error::{Error, Result};

/// Core properties shared by all Office documents (title, author, dates, …).
///
/// Dates use the W3CDTF format (e.g. `2024-01-31T12:00:00Z`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreProperties {
    /// `dc:title`
    pub title: Option<String>,
    /// `dc:subject`
    pub subject: Option<String>,
    /// `dc:creator` — the author.
    pub creator: Option<String>,
    /// `cp:keywords`
    pub keywords: Option<String>,
    /// `dc:description` — comments.
    pub description: Option<String>,
    /// `cp:lastModifiedBy`
    pub last_modified_by: Option<String>,
    /// `cp:revision`
    pub revision: Option<String>,
    /// `cp:lastPrinted`
    pub last_printed: Option<String>,
    /// `dcterms:created`
    pub created: Option<String>,
    /// `dcterms:modified`
    pub modified: Option<String>,
    /// `cp:category`
    pub category: Option<String>,
    /// `cp:contentStatus`
    pub content_status: Option<String>,
    /// `dc:language`
    pub language: Option<String>,
    /// `dc:identifier`
    pub identifier: Option<String>,
    /// `cp:version`
    pub version: Option<String>,
    /// Elements not described above, preserved as-is.
    pub extra: Vec<RawElement>,
}

macro_rules! fields {
    ($m:ident) => {
        $m!(
            (DC, "title", title),
            (DC, "subject", subject),
            (DC, "creator", creator),
            (CP, "keywords", keywords),
            (DC, "description", description),
            (CP, "lastModifiedBy", last_modified_by),
            (CP, "revision", revision),
            (CP, "lastPrinted", last_printed),
            (DCTERMS, "created", created),
            (DCTERMS, "modified", modified),
            (CP, "category", category),
            (CP, "contentStatus", content_status),
            (DC, "language", language),
            (DC, "identifier", identifier),
            (CP, "version", version)
        )
    };
}

impl CoreProperties {
    /// Parses `docProps/core.xml`.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let root = RawElement::parse_bytes(bytes)?;
        if !root.name.is(Ns::CP, "coreProperties") {
            return Err(Error::Xml(openxml_xml::Error::UnexpectedRoot {
                expected: format!("{{{}}}coreProperties", Ns::CP.uri()),
                found: format!("{{{}}}{}", root.name.uri(), root.name.local),
            }));
        }
        let mut props = CoreProperties::default();
        for el in root.elements() {
            macro_rules! assign {
                ($(($ns:ident, $local:literal, $field:ident)),*) => {
                    $(
                        if el.name.is(Ns::$ns, $local) && props.$field.is_none() {
                            props.$field = Some(el.text());
                            continue;
                        }
                    )*
                };
            }
            fields!(assign);
            props.extra.push(el.clone());
        }
        Ok(props)
    }

    /// Serializes the properties.
    pub fn to_xml(&self) -> String {
        let mut w = XmlWriter::with_declaration();
        w.predeclare(&[Ns::CP, Ns::DC, Ns::DCTERMS, Ns::DCMITYPE, Ns::XSI]);
        w.start(Ns::CP, "coreProperties");
        macro_rules! emit {
            ($(($ns:ident, $local:literal, $field:ident)),*) => {
                $(
                    if let Some(v) = &self.$field {
                        w.start(Ns::$ns, $local);
                        if Ns::$ns == Ns::DCTERMS {
                            w.attr(Ns::XSI, "type", "dcterms:W3CDTF");
                        }
                        w.text(v);
                        w.end();
                    }
                )*
            };
        }
        fields!(emit);
        for el in &self.extra {
            el.write(&mut w);
        }
        w.end();
        w.finish()
    }
}

/// Formats a Unix timestamp (seconds) as a W3CDTF UTC date-time.
pub fn w3cdtf_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The current time as a W3CDTF UTC date-time (second precision).
pub fn w3cdtf_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    w3cdtf_from_unix(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORD_CORE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>Report &amp; Plan</dc:title><dc:creator>Author</dc:creator><cp:lastModifiedBy>Editor</cp:lastModifiedBy><cp:revision>3</cp:revision><dcterms:created xsi:type="dcterms:W3CDTF">2024-01-02T03:04:05Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2024-02-03T04:05:06Z</dcterms:modified><custom xmlns="urn:x">keep</custom></cp:coreProperties>"#;

    #[test]
    fn parses_word_core_properties() {
        let p = CoreProperties::parse(WORD_CORE.as_bytes()).unwrap();
        assert_eq!(p.title.as_deref(), Some("Report & Plan"));
        assert_eq!(p.creator.as_deref(), Some("Author"));
        assert_eq!(p.last_modified_by.as_deref(), Some("Editor"));
        assert_eq!(p.revision.as_deref(), Some("3"));
        assert_eq!(p.created.as_deref(), Some("2024-01-02T03:04:05Z"));
        assert_eq!(p.modified.as_deref(), Some("2024-02-03T04:05:06Z"));
        assert_eq!(p.subject, None);
        assert_eq!(p.extra.len(), 1);
    }

    #[test]
    fn round_trips() {
        let p = CoreProperties::parse(WORD_CORE.as_bytes()).unwrap();
        let xml = p.to_xml();
        assert!(
            xml.contains(
                r#"<dcterms:created xsi:type="dcterms:W3CDTF">2024-01-02T03:04:05Z</dcterms:created>"#
            ),
            "{xml}"
        );
        assert!(xml.contains(r#"<custom xmlns="urn:x">keep</custom>"#), "{xml}");
        assert_eq!(CoreProperties::parse(xml.as_bytes()).unwrap(), p);
    }

    #[test]
    fn all_fields_round_trip() {
        let p = CoreProperties {
            title: Some("t".into()),
            subject: Some("s".into()),
            creator: Some("c".into()),
            keywords: Some("k".into()),
            description: Some("d".into()),
            last_modified_by: Some("l".into()),
            revision: Some("1".into()),
            last_printed: Some("2020-01-01T00:00:00Z".into()),
            created: Some("2020-01-01T00:00:00Z".into()),
            modified: Some("2020-01-02T00:00:00Z".into()),
            category: Some("cat".into()),
            content_status: Some("Draft".into()),
            language: Some("vi-VN".into()),
            identifier: Some("id".into()),
            version: Some("1.0".into()),
            extra: vec![],
        };
        assert_eq!(CoreProperties::parse(p.to_xml().as_bytes()).unwrap(), p);
        assert_eq!(
            CoreProperties::parse(CoreProperties::default().to_xml().as_bytes()).unwrap(),
            CoreProperties::default()
        );
    }

    #[test]
    fn rejects_wrong_root() {
        assert!(CoreProperties::parse(b"<a/>").is_err());
    }

    #[test]
    fn formats_dates() {
        assert_eq!(w3cdtf_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(w3cdtf_from_unix(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(w3cdtf_from_unix(1_709_164_799), "2024-02-28T23:59:59Z");
        assert_eq!(w3cdtf_from_unix(-1), "1969-12-31T23:59:59Z");
        assert_eq!(w3cdtf_from_unix(4_102_444_800), "2100-01-01T00:00:00Z");
        let now = w3cdtf_now();
        assert_eq!(now.len(), 20);
        assert!(now.ends_with('Z'));
    }
}
