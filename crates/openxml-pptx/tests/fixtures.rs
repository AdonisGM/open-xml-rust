//! Reading and round-tripping presentations produced by PowerPoint and other tools
//! (Apache POI test documents under `fixtures/poi`).

mod common;

use std::path::PathBuf;

use openxml_opc::Package;
use openxml_pptx::{LayoutKind, Length, PlaceholderKind, Presentation, ShapeKind};
use openxml_xml::compare::{DiffKind, semantic_diff};
use openxml_xml::{RawElement, decode_xml_bytes};

fn fixture(name: &str) -> PathBuf {
    openxml_testkit::fixture(&format!("poi/{name}"))
}

fn open(name: &str) -> Presentation {
    Presentation::open(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn all_fixtures() -> Vec<PathBuf> {
    let files: Vec<PathBuf> =
        openxml_testkit::office_files(&openxml_testkit::workspace_root().join("fixtures"))
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pptx")))
            .collect();
    assert!(
        files.len() >= 13,
        "expected the POI pptx fixtures, found {}",
        files.len()
    );
    files
}

#[test]
fn sample_show_titles_text_and_notes() {
    let deck = open("SampleShow.pptx");
    assert_eq!(deck.slide_count(), 2);
    assert_eq!(
        deck.slide_size(),
        (Length::emu(9_144_000), Length::emu(6_858_000))
    );
    let s1 = deck.slide(0).unwrap();
    assert_eq!(s1.title().as_deref(), Some("Title of the first slide"));
    assert!(s1.text().contains("Subtitle of the first slide"));
    assert!(s1.text().contains("This bit is in italic green"));
    let s2 = deck.slide(1).unwrap();
    assert_eq!(s2.title().as_deref(), Some("This is the second slide"));
    let body = s2
        .shapes()
        .into_iter()
        .find(|s| s.placeholder.is_some_and(|p| p.kind == PlaceholderKind::Object))
        .unwrap();
    assert!(
        body.text.starts_with("It has bullet points on it\n"),
        "{}",
        body.text
    );
    assert!(body.text.contains("Arial Black at 16 point!"));
    for slide in deck.slides() {
        assert!(
            slide.notes_text().is_some(),
            "{} should have notes",
            slide.part_name()
        );
    }
    assert!(deck.layouts().len() >= 11);
    assert!(deck.slides().iter().all(|s| s.layout_name().is_some()));
}

#[test]
fn layouts_deck_uses_eleven_layouts() {
    let deck = open("layouts.pptx");
    assert_eq!(deck.slide_count(), 10);
    let titles: Vec<Option<String>> = deck.slides().iter().take(6).map(|s| s.title()).collect();
    assert_eq!(titles[0].as_deref(), Some("Centered Title"));
    assert_eq!(titles[2].as_deref(), Some("Section Title"));
    assert_eq!(titles[5].as_deref(), Some("Title Only"));
    let names: Vec<&str> = deck.slides().iter().filter_map(|s| s.layout_name()).collect();
    assert!(names.contains(&"Title Slide"));
    assert!(names.contains(&"Section Header"));
    assert!(names.contains(&"Two Content"));
    assert!(
        deck.layouts()
            .iter()
            .any(|l| l.kind() == Some(LayoutKind::Comparison))
    );
    let two = deck.slide(3).unwrap();
    let idx: Vec<_> = two
        .shapes()
        .iter()
        .filter_map(|s| s.placeholder.and_then(|p| p.index))
        .collect();
    assert_eq!(idx, [1, 2]);
}

#[test]
fn shapes_deck_kinds_groups_and_tables() {
    let deck = open("shapes.pptx");
    assert_eq!(deck.slide_count(), 6);
    let first: Vec<ShapeKind> = deck.slide(0).unwrap().shapes().iter().map(|s| s.kind).collect();
    for k in [ShapeKind::AutoShape, ShapeKind::Picture, ShapeKind::Connector] {
        assert!(first.contains(&k), "{k:?} missing from {first:?}");
    }
    assert!(
        first
            .iter()
            .any(|k| matches!(k, ShapeKind::Table | ShapeKind::GraphicFrame | ShapeKind::Chart))
    );
    let group = deck
        .slide(2)
        .unwrap()
        .shapes()
        .into_iter()
        .find(|s| s.kind == ShapeKind::Group)
        .unwrap();
    assert!(!group.children.is_empty());
    assert!(group.walk().len() > group.children.len());
    let table = deck
        .slide(3)
        .unwrap()
        .shapes()
        .into_iter()
        .find(|s| s.kind == ShapeKind::Table)
        .unwrap();
    assert!(
        table.text.starts_with("header1\theader2\theader3\n"),
        "{}",
        table.text
    );
    assert!(table.text.contains("A1\tB1\tC1"));
    for info in deck.slide(0).unwrap().shapes() {
        assert!(info.id > 0);
        assert!(info.size.is_some(), "{} has a size", info.name);
    }
}

#[test]
fn charts_diagrams_and_tables_are_recognised() {
    let bar = open("bar-chart.pptx");
    let s = bar.slide(0).unwrap();
    assert_eq!(s.title().as_deref(), Some("My Bar Chart"));
    assert!(s.shapes().iter().any(|x| x.kind == ShapeKind::Chart));
    let pie = open("pie-chart.pptx");
    assert!(
        pie.slide(0)
            .unwrap()
            .shapes()
            .iter()
            .any(|x| x.kind == ShapeKind::Chart)
    );
    let smart = open("SmartArt.pptx");
    assert!(
        smart
            .slide(0)
            .unwrap()
            .shapes()
            .iter()
            .any(|x| x.kind == ShapeKind::Diagram)
    );
    assert_eq!(
        smart.slide_size(),
        (Length::emu(12_192_000), Length::emu(6_858_000))
    );
    let jp = open("with_japanese.pptx");
    let text = jp.slide(0).unwrap().text();
    assert!(text.contains("Row 1 Col 1\tRow 1 Col 2\tRow 1 Col 3"), "{text}");
    let table = open("table_test.pptx");
    assert!(
        table
            .slide(0)
            .unwrap()
            .shapes()
            .iter()
            .any(|x| x.kind == ShapeKind::Table)
    );
}

#[test]
fn masters_and_footers() {
    let themes = open("themes.pptx");
    assert_eq!(themes.slide_count(), 10);
    let masters: std::collections::HashSet<_> = themes
        .layouts()
        .iter()
        .map(|l| l.master_part_name().clone())
        .collect();
    assert_eq!(masters.len(), 7);
    assert_eq!(
        themes.slide(0).unwrap().title().as_deref(),
        Some("Standard Office theme")
    );
    let with_master = open("WithMaster.pptx");
    let footer = with_master
        .slide(1)
        .unwrap()
        .shapes()
        .into_iter()
        .find(|s| s.placeholder.is_some_and(|p| p.kind == PlaceholderKind::Footer))
        .unwrap();
    assert_eq!(footer.text, "Footer from the master slide");
}

#[test]
fn every_fixture_opens_and_describes_its_slides() {
    for path in all_fixtures() {
        let deck = Presentation::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let listed = deck
            .presentation()
            .sld_id_lst
            .as_ref()
            .map_or(0, |l| l.sld_id.len());
        assert_eq!(deck.slide_count(), listed, "{}", path.display());
        assert!(!deck.layouts().is_empty(), "{}", path.display());
        for slide in deck.slides() {
            let _ = slide.shapes();
            let _ = slide.text();
            let _ = slide.title();
            let _ = slide.notes_text();
        }
        let _ = deck.text();
    }
}

#[test]
fn unmodified_fixtures_save_byte_identical_parts() {
    for path in all_fixtures() {
        let original = Package::open_path(&path).unwrap();
        let mut deck = Presentation::open(&path).unwrap();
        let bytes = deck.to_bytes().unwrap();
        let saved = Package::from_bytes(&bytes).unwrap();
        assert_eq!(saved.part_count(), original.part_count(), "{}", path.display());
        for (name, part) in original.parts() {
            let other = saved
                .part(name)
                .unwrap_or_else(|| panic!("{}: {name} lost", path.display()));
            assert_eq!(other.data(), part.data(), "{}: {name} changed", path.display());
            assert_eq!(
                other.relationships(),
                part.relationships(),
                "{}: {name}",
                path.display()
            );
        }
    }
}

fn assert_semantically_equal(label: &str, before: &[u8], after: &[u8]) {
    let a = RawElement::parse(&decode_xml_bytes(before).unwrap()).unwrap();
    let b = RawElement::parse(&decode_xml_bytes(after).unwrap()).unwrap();
    let diffs: Vec<_> = semantic_diff(&a, &b)
        .into_iter()
        .filter(|d| d.kind != DiffKind::Reordered)
        .collect();
    assert!(diffs.is_empty(), "{label}: {diffs:#?}");
}

#[test]
fn rewritten_fixture_parts_are_semantically_unchanged() {
    for path in all_fixtures() {
        let original = Package::open_path(&path).unwrap();
        let mut deck = Presentation::open(&path).unwrap();
        // Force every typed part to be serialized again.
        let _ = deck.presentation_mut();
        for i in 0..deck.slide_count() {
            let _ = deck.slide_mut(i).unwrap().raw_mut();
        }
        let bytes = deck.to_bytes().unwrap();
        let saved = Package::from_bytes(&bytes).unwrap();
        common::assert_package_consistent(&saved);
        let rewritten: Vec<_> = std::iter::once(deck.part_name().clone())
            .chain(deck.slides().iter().map(|s| s.part_name().clone()))
            .collect();
        for (name, part) in original.parts() {
            let other = saved
                .part(name)
                .unwrap_or_else(|| panic!("{}: {name} lost", path.display()));
            if rewritten.contains(name) {
                assert_semantically_equal(&format!("{}{name}", path.display()), part.data(), other.data());
            } else if name.as_str() != "/docProps/app.xml" {
                assert_eq!(other.data(), part.data(), "{}: {name}", path.display());
            }
        }
        let reopened = Presentation::from_bytes(&bytes).unwrap();
        assert_eq!(reopened.text(), deck.text(), "{}", path.display());
    }
}

#[test]
fn fixtures_can_be_edited() {
    for path in all_fixtures() {
        let mut deck = Presentation::open(&path).unwrap();
        let before = deck.slide_count();
        let layout = deck
            .layouts()
            .iter()
            .position(|l| l.kind() == Some(LayoutKind::TitleOnly))
            .or_else(|| {
                deck.layouts()
                    .iter()
                    .position(|l| l.kind() == Some(LayoutKind::Blank))
            })
            .unwrap_or(0);
        {
            let mut s = deck.add_slide(layout).unwrap();
            s.add_text_box(
                Length::cm(1.0),
                Length::cm(1.0),
                Length::cm(10.0),
                Length::cm(2.0),
                "Added by openxml-pptx",
            );
            s.set_notes("Added notes").unwrap();
        }
        if before > 1 {
            deck.move_slide(before, 0).unwrap();
            deck.remove_slide(before).unwrap();
        }
        let bytes = deck.to_bytes().unwrap();
        let reopened = Presentation::from_bytes(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        common::assert_package_consistent(reopened.package());
        common::assert_ids_unique(&reopened);
        let expected = if before > 1 { before } else { before + 1 };
        assert_eq!(reopened.slide_count(), expected, "{}", path.display());
        let added = reopened.slide(if before > 1 { 0 } else { before }).unwrap();
        assert!(
            added.text().contains("Added by openxml-pptx"),
            "{}",
            path.display()
        );
        assert_eq!(added.notes_text().as_deref(), Some("Added notes"));
        // The parts written by this crate are schema-valid even inside third-party decks.
        let slide_xml = decode_xml_bytes(reopened.package().part(added.part_name()).unwrap().data())
            .unwrap()
            .into_owned();
        if let Err(e) = openxml_testkit::validate_xml(&slide_xml) {
            panic!("{}: new slide is invalid:\n{e}", path.display());
        }
    }
}
