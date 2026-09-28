//! The extended API on presentations produced by PowerPoint (Apache POI
//! test documents): reading formatting, themes and tables, editing, copying
//! slides between decks, and keeping untouched parts byte-identical.

mod common;

use std::collections::HashSet;
use std::path::PathBuf;

use openxml_opc::known::rel_types;
use openxml_opc::{Package, PartName};
use openxml_pptx::{
    Animation, Color, ConnectorKind, Effect, Fill, Length, Line, Link, Presentation, Rgb, SchemeColor,
    ShapeKind, ShapeType, Side, TableStyle, Transition, TransitionEffect,
};
use openxml_xml::decode_xml_bytes;

fn fixture(name: &str) -> PathBuf {
    openxml_testkit::fixture(&format!("poi/{name}"))
}

fn open(name: &str) -> Presentation {
    Presentation::open(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn pptx_fixtures() -> Vec<PathBuf> {
    openxml_testkit::office_files(&openxml_testkit::workspace_root().join("fixtures"))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pptx")))
        .collect()
}

fn xsd_valid(pkg: &Package, part: &PartName) -> bool {
    let xml = decode_xml_bytes(pkg.part(part).unwrap().data())
        .unwrap()
        .into_owned();
    openxml_testkit::validate_xml(&xml).is_ok()
}

/// Parts whose bytes are equal in both packages.
fn unchanged_parts(before: &Package, after: &Package) -> HashSet<PartName> {
    before
        .parts()
        .filter(|(n, p)| after.part(n).is_some_and(|q| q.data() == p.data()))
        .map(|(n, _)| n.clone())
        .collect()
}

/// Every part of `before` except `changed` is byte-identical in `after`.
fn assert_only_changed(label: &str, before: &Package, after: &Package, changed: &[&str]) {
    for (name, part) in before.parts() {
        if changed.contains(&name.as_str()) {
            continue;
        }
        let other = after.part(name).unwrap_or_else(|| panic!("{label}: {name} lost"));
        assert_eq!(other.data(), part.data(), "{label}: {name} changed");
    }
}

#[test]
fn backgrounds_fills_and_lines_are_read() {
    let deck = open("backgrounds.pptx");
    // Slide 2 has a four-stop gradient background of its own.
    let Some(Fill::Gradient(g)) = deck.slide(1).unwrap().background_fill() else {
        panic!("gradient background expected")
    };
    assert_eq!(g.stops.len(), 4);
    assert_eq!(g.angle, 45.0);
    assert_eq!(
        deck.slide(0).unwrap().background_fill(),
        None,
        "slide 1 uses the master"
    );
    assert_eq!(
        deck.master_background().unwrap(),
        None,
        "the master uses a style reference"
    );
    let mut deck = deck;
    let mut slide = deck.slide_mut(1).unwrap();
    let title = slide.shapes()[0].id;
    let fill = slide.shape_by_id(title).unwrap().fill_format();
    assert!(
        matches!(fill, Some(Fill::SolidAlpha(Color::Scheme(SchemeColor::Accent3), a)) if (a - 0.57).abs() < 1e-9),
        "{fill:?}"
    );
    let mut slide = deck.slide_mut(0).unwrap();
    let rect = slide
        .shapes()
        .into_iter()
        .find(|s| s.name == "Rectangle 3")
        .unwrap()
        .id;
    let shape = slide.shape_by_id(rect).unwrap();
    assert_eq!(shape.fill_format(), Some(Fill::None));
    let line = shape.line_format().unwrap();
    assert_eq!(line.width, Length::pt(10.0));
    assert_eq!(line.color, None, "gradient lines have no single colour");
    assert_eq!(shape.geometry(), Some(ShapeType::Rect));
}

#[test]
fn shapes_deck_connectors_and_freeforms() {
    let path = fixture("shapes.pptx");
    let original = Package::open_path(&path).unwrap();
    let mut deck = Presentation::open(&path).unwrap();
    let slide = deck.slide(0).unwrap();
    let shapes = slide.shapes();
    let connector = shapes.iter().find(|s| s.name == "Straight Connector 5").unwrap();
    assert_eq!(connector.kind, ShapeKind::Connector);
    let freeform = shapes.iter().find(|s| s.name == "Freeform 6").unwrap().id;
    let text_box = shapes.iter().find(|s| s.name == "TextBox 3").unwrap().id;
    let was_valid = xsd_valid(&original, slide.part_name());
    let part = slide.part_name().clone();

    let mut slide = deck.slide_mut(0).unwrap();
    {
        let mut f = slide.shape_by_id(freeform).unwrap();
        assert_eq!(f.geometry(), None, "custom geometry");
        f.set_fill(Fill::solid(Rgb(0x80, 0xC0, 0x40))).set_rotation(10.0);
    }
    let cxn = slide
        .connect_shapes(
            ConnectorKind::Straight,
            text_box,
            Side::Bottom,
            freeform,
            Side::Top,
        )
        .unwrap()
        .id();
    slide
        .add_animation(freeform, Animation::new(Effect::FadeIn))
        .unwrap();
    let bytes = deck.to_bytes().unwrap();
    let saved = Package::from_bytes(&bytes).unwrap();
    assert_only_changed("shapes.pptx", &original, &saved, &[part.as_str()]);
    if was_valid {
        assert!(xsd_valid(&saved, &part), "the edited slide stays schema-valid");
    }
    common::assert_rust_valid(&saved, |n| n == &part);
    let mut deck = Presentation::from_bytes(&bytes).unwrap();
    common::assert_ids_unique(&deck);
    let mut slide = deck.slide_mut(0).unwrap();
    assert_eq!(
        slide.connector_mut(cxn).unwrap().end_connection().map(|c| c.0),
        Some(freeform)
    );
    assert_eq!(slide.shape_by_id(freeform).unwrap().rotation(), 10.0);
    assert_eq!(slide.animations().len(), 1);
}

#[test]
fn highlighted_text_is_read() {
    let mut deck = open("text-highlight.pptx");
    let mut slide = deck.slide_mut(0).unwrap();
    let ids: Vec<u32> = slide.shapes().iter().map(|s| s.id).collect();
    let mut colors = Vec::new();
    for id in ids {
        if let Some(mut s) = slide.shape_by_id(id)
            && let Some(mut p) = s.paragraph_mut(0)
            && let Some(r) = p.run_mut(0)
        {
            colors.push(r.highlight_color());
        }
    }
    assert!(colors.contains(&Some(Color::Rgb(Rgb(0xFF, 0, 0)))), "{colors:?}");
    assert!(
        colors.contains(&Some(Color::Scheme(SchemeColor::Accent4))),
        "{colors:?}"
    );
}

#[test]
fn table_fixture_is_read_and_edited() {
    let path = fixture("table_test.pptx");
    let original = Package::open_path(&path).unwrap();
    let mut deck = Presentation::open(&path).unwrap();
    let part = deck.slide(0).unwrap().part_name().clone();
    let id = deck
        .slide(0)
        .unwrap()
        .shapes()
        .into_iter()
        .find(|s| s.kind == ShapeKind::Table)
        .unwrap()
        .id;
    let mut slide = deck.slide_mut(0).unwrap();
    let mut t = slide.table_mut(id).unwrap();
    assert_eq!(t.style(), Some(TableStyle::MediumStyle2Accent1));
    assert!(t.flags().first_row && t.flags().banded_rows && !t.flags().last_row);
    assert_eq!(t.column_width(1), Some(Length::emu(3_829_642)));
    let rows = t.rows();
    t.set_cell_text(0, 0, "Name").unwrap();
    t.merge_cells(1, 0, 2, 0).unwrap();
    t.set_cell_fill(0, 1, Fill::solid(Rgb(0xFF, 0xF2, 0xCC))).unwrap();
    t.set_cell_borders(0, 2, Line::solid(Rgb::BLACK, Length::pt(2.0)))
        .unwrap();
    t.insert_row(rows).unwrap();
    t.set_style(TableStyle::MediumStyle2);
    let bytes = deck.to_bytes().unwrap();
    let saved = Package::from_bytes(&bytes).unwrap();
    assert_only_changed(
        "table_test.pptx",
        &original,
        &saved,
        &[part.as_str(), "/docProps/app.xml"],
    );
    assert!(xsd_valid(&saved, &part));
    common::assert_rust_valid(&saved, |n| n == &part);
    let mut deck = Presentation::from_bytes(&bytes).unwrap();
    let mut slide = deck.slide_mut(0).unwrap();
    let t = slide.table_mut(id).unwrap();
    assert_eq!(t.rows(), rows + 1);
    assert_eq!(t.cell_span(1, 0), Some((2, 1, false)));
    assert_eq!(t.cell_text(0, 0).as_deref(), Some("Name"));
    assert_eq!(t.style(), Some(TableStyle::MediumStyle2));
}

#[test]
fn themes_of_powerpoint_decks() {
    let deck = open("WithMaster.pptx");
    let colors = deck.theme_colors().unwrap();
    assert_eq!(colors.dark1, Rgb::BLACK, "system colours report their last value");
    assert_eq!(colors.light1, Rgb::WHITE);
    assert_eq!(colors.accent1, Rgb(0x4F, 0x81, 0xBD));
    assert_eq!(colors.followed_hyperlink, Rgb(0x80, 0x00, 0x80));
    let fonts = deck.theme_fonts().unwrap();
    assert_eq!(fonts.major.latin, "Calibri");

    // A deck with seven masters: every theme is updated, nothing else changes.
    let path = fixture("themes.pptx");
    let original = Package::open_path(&path).unwrap();
    let mut deck = Presentation::open(&path).unwrap();
    let mut colors = deck.theme_colors().unwrap();
    colors.accent2 = Rgb(1, 2, 3);
    deck.set_theme_colors(&colors, None).unwrap();
    let bytes = deck.to_bytes().unwrap();
    let saved = Package::from_bytes(&bytes).unwrap();
    let unchanged = unchanged_parts(&original, &saved);
    for (name, _) in original.parts() {
        let is_theme = name.as_str().starts_with("/ppt/theme/theme") && name.as_str().ends_with(".xml");
        assert_eq!(!unchanged.contains(name), is_theme, "{name}");
    }
    let reopened = Presentation::from_bytes(&bytes).unwrap();
    assert_eq!(reopened.theme_colors().unwrap().accent2, Rgb(1, 2, 3));
    common::assert_package_consistent(reopened.package());
}

#[test]
fn smartart_slides_are_duplicated_with_their_diagram() {
    let mut deck = open("SmartArt.pptx");
    let original_parts = deck.package().part_count();
    let copy = deck.duplicate_slide(0).unwrap();
    let deck = common::round_trip_consistent(&mut deck);
    let (a, b) = (deck.slide(0).unwrap(), deck.slide(copy).unwrap());
    assert_eq!(a.text(), b.text());
    let pkg = deck.package();
    for kind in [
        rel_types::DIAGRAM_DATA,
        rel_types::DIAGRAM_LAYOUT,
        rel_types::DIAGRAM_QUICK_STYLE,
        rel_types::DIAGRAM_COLORS,
        "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing",
    ] {
        let pa = pkg.related_part(Some(a.part_name()), kind).unwrap();
        let pb = pkg.related_part(Some(b.part_name()), kind).unwrap();
        assert_ne!(pa, pb, "{kind} is deep-copied");
        assert_eq!(pkg.part(&pa).unwrap().data(), pkg.part(&pb).unwrap().data());
    }
    // Slide, rels-less diagram parts (5) and the slide itself were added.
    assert_eq!(pkg.part_count(), original_parts + 6);
    assert!(xsd_valid(pkg, b.part_name()) == xsd_valid(pkg, a.part_name()));
}

/// Duplicates every slide of every fixture and imports every slide into a
/// new presentation; packages stay consistent and schema-valid slides stay valid.
#[test]
fn every_fixture_slide_can_be_duplicated_and_imported() {
    for path in pptx_fixtures() {
        let label = path.display().to_string();
        let source = Presentation::open(&path).unwrap();
        let n = source.slide_count();

        let mut dup = Presentation::open(&path).unwrap();
        for i in (0..n).rev() {
            dup.duplicate_slide(i)
                .unwrap_or_else(|e| panic!("{label}: duplicate {i}: {e}"));
        }
        let bytes = dup.to_bytes().unwrap();
        let dup = Presentation::from_bytes(&bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
        common::assert_package_consistent(dup.package());
        common::assert_ids_unique(&dup);
        assert_eq!(dup.slide_count(), 2 * n, "{label}");
        for i in 0..n {
            let (a, b) = (dup.slide(2 * i).unwrap(), dup.slide(2 * i + 1).unwrap());
            assert_eq!(a.text(), b.text(), "{label}: slide {i}");
            assert_eq!(a.notes_text(), b.notes_text(), "{label}: notes {i}");
            if xsd_valid(source.package(), source.slide(i).unwrap().part_name()) {
                assert!(
                    xsd_valid(dup.package(), b.part_name()),
                    "{label}: copy of slide {i}"
                );
            }
        }

        let mut target = Presentation::new();
        for i in 0..n {
            target
                .import_slide(&source, i)
                .unwrap_or_else(|e| panic!("{label}: import {i}: {e}"));
        }
        let bytes = target.to_bytes().unwrap();
        let target = Presentation::from_bytes(&bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
        common::assert_package_consistent(target.package());
        common::assert_ids_unique(&target);
        assert_eq!(target.slide_count(), n);
        for i in 0..n {
            let (a, b) = (source.slide(i).unwrap(), target.slide(i).unwrap());
            assert_eq!(a.text(), b.text(), "{label}: imported slide {i}");
            assert_eq!(a.notes_text(), b.notes_text(), "{label}: imported notes {i}");
            if xsd_valid(source.package(), a.part_name()) {
                assert!(
                    xsd_valid(target.package(), b.part_name()),
                    "{label}: imported slide {i}"
                );
            }
        }
        // The parts the target presentation itself defines stay valid (copied
        // parts such as charts carry the source's own quirks).
        common::assert_rust_valid(target.package(), |n| {
            let n = n.as_str();
            n == "/ppt/presentation.xml"
                || [
                    "/ppt/slideMasters/",
                    "/ppt/slideLayouts/",
                    "/ppt/theme/",
                    "/ppt/notesMasters/",
                ]
                .iter()
                .any(|p| n.starts_with(p))
        });
    }
}

#[test]
fn interactive_features_on_a_powerpoint_deck() {
    let path = fixture("SampleShow.pptx");
    let original = Package::open_path(&path).unwrap();
    let mut deck = Presentation::open(&path).unwrap();
    let part = deck.slide(1).unwrap().part_name().clone();
    let mut slide = deck.slide_mut(1).unwrap();
    slide.set_transition(Some(Transition::new(TransitionEffect::Fade {
        through_black: true,
    })));
    let id = slide
        .add_shape(
            ShapeType::ActionButtonBackPrevious,
            Length::cm(1.0),
            Length::cm(15.0),
            Length::cm(2.0),
            Length::cm(2.0),
        )
        .id();
    slide.set_link(id, Some(Link::PreviousSlide)).unwrap();
    slide.add_animation(id, Animation::new(Effect::Appear)).unwrap();
    let bytes = deck.to_bytes().unwrap();
    let saved = Package::from_bytes(&bytes).unwrap();
    assert_only_changed("SampleShow.pptx", &original, &saved, &[part.as_str()]);
    assert!(xsd_valid(&saved, &part));
    let deck = Presentation::from_bytes(&bytes).unwrap();
    assert_eq!(deck.slide_links(1)[0].link, Link::PreviousSlide);
    assert!(deck.slide(1).unwrap().transition().is_some());
}
