//! Reading and round-tripping real Word documents (fixtures/poi, Apache POI test data).

use std::path::{Path, PathBuf};

use openxml_docx::{Document, HeaderFooterKind, Length, ListKind};
use openxml_opc::known::rel_types;
use openxml_opc::{Package, PartName};
use openxml_testkit::{fixture, office_files, workspace_root};
use openxml_xml::compare::{DiffKind, semantic_diff};
use openxml_xml::{RawElement, decode_xml_bytes};

fn docx_fixtures() -> Vec<PathBuf> {
    let files: Vec<PathBuf> = office_files(&workspace_root().join("fixtures"))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("docx")))
        .collect();
    assert!(
        files.len() >= 15,
        "expected the POI .docx fixtures, found {}",
        files.len()
    );
    files
}

fn open(name: &str) -> Document {
    Document::open(fixture(&format!("poi/{name}"))).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn raw(pkg: &Package, name: &PartName) -> RawElement {
    RawElement::parse(&decode_xml_bytes(pkg.part(name).unwrap().data()).unwrap()).unwrap()
}

fn assert_semantically_equal(before: &Package, after: &Package, name: &PartName, file: &Path) {
    let diffs: Vec<_> = semantic_diff(&raw(before, name), &raw(after, name))
        .into_iter()
        .filter(|d| d.kind != DiffKind::Reordered)
        .collect();
    assert!(diffs.is_empty(), "{}{name}: {diffs:#?}", file.display());
}

#[test]
fn every_fixture_opens_and_extracts_text() {
    for path in docx_fixtures() {
        let doc = Document::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        // Text extraction must work on every document; most have body content.
        let text = doc.text();
        let blocks = doc.blocks().len();
        assert!(blocks > 0 || text.is_empty(), "{}", path.display());
        for hf in doc.headers_and_footers() {
            let _ = hf.text();
        }
    }
}

#[test]
fn unmodified_documents_round_trip_byte_for_byte() {
    for path in docx_fixtures() {
        let original = Package::open_path(&path).unwrap();
        let mut doc = Document::open(&path).unwrap();
        let saved = Package::from_bytes(&doc.to_bytes().unwrap()).unwrap();
        assert_eq!(saved, original, "{}", path.display());
    }
}

#[test]
fn normalized_documents_are_semantically_equal() {
    for path in docx_fixtures() {
        let original = Package::open_path(&path).unwrap();
        let mut doc = Document::open(&path).unwrap();
        let typed: Vec<PartName> = {
            let main = doc.main_part_name().clone();
            let mut v = vec![main.clone()];
            for rel in [
                rel_types::STYLES,
                rel_types::NUMBERING,
                rel_types::HEADER,
                rel_types::FOOTER,
                rel_types::SETTINGS,
                rel_types::COMMENTS,
                rel_types::FOOTNOTES,
                rel_types::ENDNOTES,
            ] {
                v.extend(
                    original
                        .related_parts(Some(&main), rel)
                        .into_iter()
                        .filter(|p| original.contains(p)),
                );
            }
            v
        };
        doc.normalize();
        let saved = Package::from_bytes(&doc.to_bytes().unwrap()).unwrap();
        assert_eq!(saved.part_count(), original.part_count(), "{}", path.display());
        for (name, part) in original.parts() {
            let other = saved
                .part(name)
                .unwrap_or_else(|| panic!("{}{name} missing", path.display()));
            assert_eq!(
                other.relationships(),
                part.relationships(),
                "{}{name}",
                path.display()
            );
            if typed.contains(name) {
                assert_semantically_equal(&original, &saved, name, &path);
            } else {
                assert_eq!(
                    other.data(),
                    part.data(),
                    "{}{name} must not change",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn edits_leave_untouched_parts_identical() {
    for name in [
        "sample.docx",
        "Headers.docx",
        "Numbering.docx",
        "VariousPictures.docx",
        "delins.docx",
    ] {
        let path = fixture(&format!("poi/{name}"));
        let original = Package::open_path(&path).unwrap();
        let mut doc = Document::open(&path).unwrap();
        let before = doc.text();
        doc.add_paragraph("Appended by openxml-docx")
            .add_run(" (bold)")
            .bold(true);
        doc.add_list_item("new item", ListKind::Bullet, 0).unwrap();
        let bytes = doc.to_bytes().unwrap();
        let saved = Package::from_bytes(&bytes).unwrap();
        let main = doc.main_part_name().clone();
        let numbering = saved.related_part(Some(&main), rel_types::NUMBERING);
        let styles = saved.related_part(Some(&main), rel_types::STYLES);
        for (part_name, part) in original.parts() {
            if *part_name == main
                || Some(part_name) == numbering.as_ref()
                || Some(part_name) == styles.as_ref()
            {
                continue;
            }
            assert_eq!(
                saved.part(part_name).unwrap().data(),
                part.data(),
                "{name}{part_name}"
            );
        }
        let back = Document::from_bytes(&bytes).unwrap();
        let expected = if before.is_empty() {
            "Appended by openxml-docx (bold)\nnew item".to_owned()
        } else {
            format!("{before}\nAppended by openxml-docx (bold)\nnew item")
        };
        assert_eq!(back.text(), expected, "{name}");
        assert!(back.paragraphs().last().unwrap().numbering().is_some());
    }
}

#[test]
fn sample_document_text_and_headers() {
    let doc = open("sample.docx");
    let ps = doc.paragraphs();
    assert_eq!(ps.len(), 3);
    assert!(
        ps[0]
            .text()
            .starts_with("Lorem ipsum dolor sit amet, consectetuer adipiscing elit.")
    );
    assert!(ps[1].text().starts_with("Nullam sapien."));
    let headers = doc.headers_and_footers();
    assert_eq!(
        headers
            .iter()
            .filter(|h| h.kind() == HeaderFooterKind::Header)
            .count(),
        3
    );
    assert_eq!(
        headers
            .iter()
            .filter(|h| h.kind() == HeaderFooterKind::Footer)
            .count(),
        3
    );
}

#[test]
fn headers_of_each_section_are_read() {
    let doc = open("Headers.docx");
    let mut texts: Vec<String> = doc
        .headers_and_footers()
        .iter()
        .map(|h| h.text().trim_end().to_owned())
        .collect();
    texts.sort();
    assert_eq!(texts, ["Section 1", "Section 2", "Section 3"]);
    // The last section's default header.
    assert_eq!(doc.header().unwrap().text().trim_end(), "Section 3");
}

#[test]
fn numbering_definitions_and_list_paragraphs() {
    let doc = open("Numbering.docx");
    let numbering = doc.numbering().unwrap();
    assert_eq!(numbering.abstract_num.len(), 5);
    assert_eq!(numbering.num.len(), 5);
    let numbered: Vec<_> = doc
        .paragraphs()
        .into_iter()
        .filter(|p| p.numbering().is_some())
        .collect();
    assert!(numbered.len() >= 12, "{}", numbered.len());
    assert_eq!(numbered[0].text(), "Level 1");
    assert_eq!(numbered[1].numbering().unwrap().1, 1, "second item is on level 2");
}

#[test]
fn tracked_deletions_are_not_text_but_insertions_are() {
    let doc = open("delins.docx");
    let text = doc.text();
    assert!(
        text.starts_with("Tika can be:\nA Nepalese name for Tilaka"),
        "{text}"
    );
    assert!(
        !text.contains("A pendant worn in place of the red spot"),
        "deleted text must be excluded"
    );
    assert!(!text.contains("Tika Waylan"));
    assert!(text.contains("Apache Tika 0.3 has been released."));
}

#[test]
fn right_to_left_and_other_scripts() {
    let doc = open("rtl.docx");
    assert!(doc.text().starts_with("إسبانيا (الإسبانية: España)"));
    assert_eq!(open("footnotes.docx").text(), "Eto ochen prostoy text so snoskoy");
    assert!(
        open("bookmarks.docx")
            .text()
            .starts_with("Sample Word Document\n")
    );
}

#[test]
fn field_results_are_text_but_instructions_are_not() {
    let text = open("FieldCodes.docx").text();
    assert!(text.starts_with("ANTONI\n16 June 2010"), "{text:?}");
    assert!(!text.contains("AUTHOR") && !text.contains("DATE"));
}

#[test]
fn tables_in_real_documents() {
    let doc = open("checkboxes.docx");
    assert_eq!(doc.tables().len(), 1);
    let table = doc.tables()[0];
    assert!(table.row_count() >= 1);
    assert!(doc.text().contains("In Table:"));
}

#[test]
fn pictures_in_real_documents() {
    let mut doc = open("VariousPictures.docx");
    let drawings = doc
        .paragraphs()
        .iter()
        .flat_map(|p| p.runs())
        .filter(|r| r.has_drawing())
        .count();
    assert!(drawings >= 1);
    let main = doc.main_part_name().clone();
    let before = doc.package().related_parts(Some(&main), rel_types::IMAGE).len();
    assert_eq!(before, 5);
    // New pictures get fresh part names and drawing ids.
    doc.add_picture(&openxml_core::image::tiny_png(10, 10), Length::inches(1.0))
        .unwrap();
    let bytes = doc.to_bytes().unwrap();
    let back = Document::from_bytes(&bytes).unwrap();
    let images = back.package().related_parts(Some(&main), rel_types::IMAGE);
    assert_eq!(images.len(), 6);
    // image1.wmf … image5.jpeg exist; the smallest free PNG name is image1.png.
    assert!(
        images.contains(&PartName::new("/word/media/image1.png").unwrap()),
        "{images:?}"
    );
    let original = Package::open_path(fixture("poi/VariousPictures.docx")).unwrap();
    assert!(!original.contains(&PartName::new("/word/media/image1.png").unwrap()));
}

#[test]
fn localized_styles_of_a_german_document_are_reused() {
    // Styles.docx was written by a German Word: style ids are localized
    // ("Standard", "berschrift1") while the names are the built-in ones.
    let mut doc = open("Styles.docx");
    let ids = doc.style_ids();
    assert!(
        ids.contains(&"Standard") && ids.contains(&"berschrift1"),
        "{ids:?}"
    );
    assert!(!ids.contains(&"Heading1"));
    doc.add_heading("Kapitel", 1).unwrap();
    assert_eq!(doc.paragraphs().last().unwrap().style_id(), Some("berschrift1"));
    doc.add_paragraph("")
        .add_hyperlink("Link", "https://example.de")
        .unwrap();
    doc.add_table(1, 1).unwrap();
    let bytes = doc.to_bytes().unwrap();
    let back = Document::from_bytes(&bytes).unwrap();
    let styles = back.styles().unwrap();
    let based_on = |id: &str| {
        styles
            .style
            .iter()
            .find(|s| s.style_id.as_deref() == Some(id))
            .and_then(|s| s.based_on.as_ref())
            .and_then(|b| b.val.clone())
    };
    assert_eq!(
        based_on("TableGrid").as_deref(),
        Some("NormaleTabelle"),
        "based on the localized default"
    );
    assert_eq!(
        based_on("Hyperlink").as_deref(),
        Some("Absatz-Standardschriftart")
    );
    assert!(
        !back.style_ids().contains(&"Normal"),
        "no duplicate default paragraph style"
    );
    // Exactly one default style per type.
    for ty in ["paragraph", "character", "table"] {
        let defaults = styles
            .style
            .iter()
            .filter(|s| s.type_.map(|t| t.to_string()) == Some(ty.to_owned()) && s.default.is_some())
            .count();
        assert_eq!(defaults, 1, "{ty}");
    }
}
