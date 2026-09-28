//! Creating content: every editing feature is saved, reopened, validated and checked.

mod common;

use openxml_core::image::tiny_png;
use openxml_opc::known::rel_types;
use openxml_opc::{Package, PartName};
use openxml_pptx::{
    Alignment, Error, FontSize, LayoutKind, LayoutRef, Length, PlaceholderKind, Presentation, Rgb, ShapeKind,
};
use openxml_schema::{dml, pml};

fn save_and_check(deck: &mut Presentation) -> Presentation {
    let bytes = deck.to_bytes().unwrap();
    common::check_saved(&bytes)
}

#[test]
fn validation_detects_broken_parts() {
    // Negative control: the validation helper must fail on an invalid slide.
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Blank).unwrap();
    let mut bytes_deck = deck.clone();
    let bytes = bytes_deck.to_bytes().unwrap();
    let mut pkg = Package::from_bytes(&bytes).unwrap();
    let slide = PartName::new("/ppt/slides/slide1.xml").unwrap();
    pkg.set_part_data(
        &slide,
        br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:bogus/></p:sld>"#
            .to_vec(),
    )
    .unwrap();
    assert!(!openxml_testkit::validate_package(&pkg).is_empty());
}

#[test]
fn titles_subtitles_and_body_text() {
    let mut deck = Presentation::new();
    {
        let mut s = deck.add_slide(LayoutKind::Title).unwrap();
        s.set_title("Main title").unwrap();
        s.set_subtitle("A subtitle").unwrap();
    }
    {
        let mut s = deck.add_slide(LayoutKind::TitleAndContent).unwrap();
        s.set_title("Agenda").unwrap();
        s.set_body_text(&["First", "Second", "Third"]).unwrap();
    }
    {
        let mut s = deck.add_slide(LayoutKind::TitleAndContent).unwrap();
        s.set_title("Levels").unwrap();
        s.set_body_levels(&[(0, "Top"), (1, "Nested"), (2, "Deeper")])
            .unwrap();
    }
    {
        let mut s = deck.add_slide(LayoutKind::SectionHeader).unwrap();
        s.set_title("Part two").unwrap();
        s.set_body_text(&["Section description"]).unwrap();
    }
    let deck = save_and_check(&mut deck);
    let s0 = deck.slide(0).unwrap();
    assert_eq!(s0.title().as_deref(), Some("Main title"));
    assert_eq!(s0.text(), "Main title\nA subtitle");
    let s1 = deck.slide(1).unwrap();
    assert_eq!(s1.title().as_deref(), Some("Agenda"));
    assert_eq!(s1.text(), "Agenda\nFirst\nSecond\nThird");
    let s2 = deck.slide(2).unwrap();
    let body = s2
        .shapes()
        .into_iter()
        .find(|s| s.placeholder.is_some_and(|p| p.kind == PlaceholderKind::Object))
        .unwrap();
    assert_eq!(body.text, "Top\nNested\nDeeper");
    let pml::CT_GroupShape_Choice::Sp(sp) =
        &s2.raw().c_sld.as_ref().unwrap().sp_tree.as_ref().unwrap().choice[1]
    else {
        panic!()
    };
    let levels: Vec<Option<i32>> = sp
        .tx_body
        .as_ref()
        .unwrap()
        .p
        .iter()
        .map(|p| p.p_pr.as_ref().and_then(|pp| pp.lvl))
        .collect();
    assert_eq!(levels, [None, Some(1), Some(2)]);
    assert_eq!(deck.slide(3).unwrap().text(), "Part two\nSection description");
}

#[test]
fn missing_placeholders_are_restored_from_the_layout_or_reported() {
    let mut deck = Presentation::new();
    {
        let mut s = deck.add_slide(LayoutKind::TitleAndContent).unwrap();
        // Delete the title shape, then set the title again: it is re-created from the layout.
        let title_id = s
            .shapes()
            .into_iter()
            .find(|s| s.placeholder.is_some_and(|p| p.kind.is_title()))
            .unwrap()
            .id;
        assert!(s.remove_shape(title_id));
        assert!(!s.remove_shape(999));
        assert!(s.title().is_none());
        s.set_title("Restored").unwrap();
        assert_eq!(s.title().as_deref(), Some("Restored"));
    }
    {
        let mut s = deck.add_slide(LayoutKind::Blank).unwrap();
        assert!(matches!(s.set_title("x"), Err(Error::NotFound(_))));
        assert!(matches!(s.set_subtitle("x"), Err(Error::NotFound(_))));
        assert!(matches!(s.set_body_text(&["x"]), Err(Error::NotFound(_))));
        // Footer-area placeholders exist on the layout and can be requested explicitly.
        s.placeholder_mut(PlaceholderKind::Footer)
            .unwrap()
            .set_text("Confidential");
    }
    let deck = save_and_check(&mut deck);
    assert_eq!(deck.slide(0).unwrap().title().as_deref(), Some("Restored"));
    let footer = deck.slide(1).unwrap().shapes();
    assert_eq!(footer.len(), 1);
    assert_eq!(footer[0].placeholder.unwrap().kind, PlaceholderKind::Footer);
    assert_eq!(footer[0].text, "Confidential");
}

#[test]
fn text_boxes_with_formatting() {
    let mut deck = Presentation::new();
    {
        let mut s = deck.add_slide(LayoutKind::Blank).unwrap();
        let mut b = s.add_text_box(
            Length::cm(1.0),
            Length::cm(2.0),
            Length::cm(10.0),
            Length::cm(3.0),
            "Hello\nWorld",
        );
        b.font_size(FontSize(24.0))
            .bold(true)
            .italic(true)
            .underline(true)
            .color(Rgb(0xC0, 0x00, 0x00))
            .font("Georgia")
            .align(Alignment::Right)
            .fill(Rgb(0xEE, 0xEE, 0xEE))
            .outline(Rgb::BLACK, Length::pt(2.0))
            .set_name("Greeting");
        assert!(b.id() >= 2);
        s.add_text_box(
            Length::cm(1.0),
            Length::cm(6.0),
            Length::cm(5.0),
            Length::cm(1.0),
            "Second",
        );
    }
    let deck = save_and_check(&mut deck);
    let slide = deck.slide(0).unwrap();
    let shapes = slide.shapes();
    assert_eq!(shapes.len(), 2);
    let greeting = &shapes[0];
    assert_eq!(greeting.kind, ShapeKind::TextBox);
    assert_eq!(greeting.name, "Greeting");
    assert_eq!(greeting.text, "Hello\nWorld");
    assert_eq!(greeting.offset, Some((Length::cm(1.0), Length::cm(2.0))));
    assert_eq!(greeting.size, Some((Length::cm(10.0), Length::cm(3.0))));
    assert_ne!(shapes[0].id, shapes[1].id);
    let pml::CT_GroupShape_Choice::Sp(sp) = &slide
        .raw()
        .c_sld
        .as_ref()
        .unwrap()
        .sp_tree
        .as_ref()
        .unwrap()
        .choice[0]
    else {
        panic!()
    };
    let body = sp.tx_body.as_ref().unwrap();
    let dml::EG_TextRun::R(run) = &body.p[1].text_run[0] else {
        panic!()
    };
    let props = run.r_pr.as_ref().unwrap();
    assert_eq!(props.sz, Some(2400));
    assert_eq!((props.b, props.i), (Some(true), Some(true)));
    assert_eq!(props.u, Some(dml::ST_TextUnderlineType::Sng));
    assert_eq!(props.latin.as_ref().unwrap().typeface.as_deref(), Some("Georgia"));
    assert!(matches!(
        &props.fill_properties,
        Some(dml::EG_FillProperties::SolidFill(_))
    ));
    assert_eq!(
        body.p[0].p_pr.as_ref().unwrap().algn,
        Some(dml::ST_TextAlignType::R)
    );
    let sp_pr = sp.sp_pr.as_ref().unwrap();
    assert!(matches!(
        &sp_pr.fill_properties,
        Some(dml::EG_FillProperties::SolidFill(_))
    ));
    assert_eq!(sp_pr.ln.as_ref().unwrap().w, Some(25_400));
}

#[test]
fn editing_existing_shapes_by_name() {
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Blank)
        .unwrap()
        .add_text_box(
            Length::cm(1.0),
            Length::cm(1.0),
            Length::cm(5.0),
            Length::cm(1.0),
            "old",
        )
        .set_name("Label");
    let bytes = deck.to_bytes().unwrap();
    let mut deck = Presentation::from_bytes(&bytes).unwrap();
    {
        let mut s = deck.slide_mut(0).unwrap();
        assert!(s.shape_mut("Nope").is_none());
        let mut label = s.shape_mut("Label").unwrap();
        assert_eq!(label.text(), "old");
        label
            .set_text("new")
            .set_position(Length::cm(2.0), Length::cm(3.0))
            .set_size(Length::cm(4.0), Length::cm(2.0));
        assert_eq!(
            label
                .raw()
                .nv_sp_pr
                .as_ref()
                .unwrap()
                .c_nv_pr
                .as_ref()
                .unwrap()
                .name
                .as_deref(),
            Some("Label")
        );
    }
    let deck = save_and_check(&mut deck);
    let info = &deck.slide(0).unwrap().shapes()[0];
    assert_eq!(info.text, "new");
    assert_eq!(info.offset, Some((Length::cm(2.0), Length::cm(3.0))));
    assert_eq!(info.size, Some((Length::cm(4.0), Length::cm(2.0))));
}

#[test]
fn pictures_are_embedded_once_and_sized_by_aspect_ratio() {
    let png = tiny_png(200, 100);
    let other = tiny_png(10, 10);
    let mut deck = Presentation::new();
    {
        let mut s = deck.add_slide(LayoutKind::Blank).unwrap();
        let a = s
            .add_picture(&png, Length::cm(1.0), Length::cm(1.0), Length::cm(10.0), None)
            .unwrap();
        let b = s
            .add_picture(
                &png,
                Length::cm(12.0),
                Length::cm(1.0),
                Length::cm(4.0),
                Some(Length::cm(4.0)),
            )
            .unwrap();
        assert_ne!(a, b);
        assert!(matches!(
            s.add_picture(b"not an image", Length::ZERO, Length::ZERO, Length::cm(1.0), None),
            Err(Error::UnsupportedImage)
        ));
    }
    deck.add_slide(LayoutKind::Blank)
        .unwrap()
        .add_picture(&png, Length::ZERO, Length::ZERO, Length::cm(2.0), None)
        .unwrap();
    deck.add_slide(LayoutKind::Blank)
        .unwrap()
        .add_picture(&other, Length::ZERO, Length::ZERO, Length::cm(2.0), None)
        .unwrap();
    let deck = save_and_check(&mut deck);
    let media: Vec<String> = deck
        .package()
        .parts()
        .filter(|(n, _)| n.as_str().starts_with("/ppt/media/"))
        .map(|(n, _)| n.to_string())
        .collect();
    assert_eq!(
        media,
        ["/ppt/media/image1.png", "/ppt/media/image2.png"],
        "identical images are stored once"
    );
    let s0 = deck.slide(0).unwrap();
    let pics = s0.shapes();
    assert!(pics.iter().all(|p| p.kind == ShapeKind::Picture));
    assert_eq!(
        pics[0].size,
        Some((Length::cm(10.0), Length::cm(5.0))),
        "height follows the 2:1 aspect ratio"
    );
    assert_eq!(pics[1].size, Some((Length::cm(4.0), Length::cm(4.0))));
    let rels = deck.package().relationships(Some(s0.part_name())).unwrap();
    assert_eq!(
        rels.by_type(rel_types::IMAGE).count(),
        1,
        "one relationship per slide and image"
    );
    let pml::CT_GroupShape_Choice::Pic(pic) =
        &s0.raw().c_sld.as_ref().unwrap().sp_tree.as_ref().unwrap().choice[0]
    else {
        panic!()
    };
    let rid = pic
        .blip_fill
        .as_ref()
        .unwrap()
        .blip
        .as_ref()
        .unwrap()
        .r_embed
        .clone()
        .unwrap();
    let target = deck
        .package()
        .relationship_target(Some(s0.part_name()), &rid)
        .unwrap();
    assert_eq!(deck.package().part(&target).unwrap().data(), &png[..]);
    assert_eq!(deck.package().part(&target).unwrap().content_type(), "image/png");
}

#[test]
fn tables_with_text() {
    let mut deck = Presentation::new();
    let table_id;
    {
        let mut s = deck.add_slide(LayoutKind::TitleOnly).unwrap();
        s.set_title("Numbers").unwrap();
        let mut t = s
            .add_table(
                3,
                2,
                Length::cm(2.0),
                Length::cm(5.0),
                Length::cm(16.0),
                Length::cm(6.0),
            )
            .unwrap();
        t.set_values([["Name", "Value"], ["alpha", "1"], ["beta", "2"]])
            .unwrap();
        t.set_column_width(0, Length::cm(10.0)).unwrap();
        t.set_header_row(true);
        table_id = t.id();
        assert!(
            s.add_table(0, 2, Length::ZERO, Length::ZERO, Length::cm(1.0), Length::cm(1.0))
                .is_err()
        );
    }
    let mut deck = save_and_check(&mut deck);
    let slide = deck.slide(0).unwrap();
    let table = slide
        .shapes()
        .into_iter()
        .find(|s| s.kind == ShapeKind::Table)
        .unwrap();
    assert_eq!(table.id, table_id);
    assert_eq!(table.text, "Name\tValue\nalpha\t1\nbeta\t2");
    assert_eq!(table.size, Some((Length::cm(16.0), Length::cm(6.0))));
    assert_eq!(slide.text(), "Numbers\nName\tValue\nalpha\t1\nbeta\t2");
    {
        let mut s = deck.slide_mut(0).unwrap();
        assert!(s.table_mut(12345).is_none());
        let mut t = s.table_mut(table_id).unwrap();
        assert_eq!((t.rows(), t.cols()), (3, 2));
        t.set_cell_text(2, 1, "20").unwrap();
    }
    let deck = save_and_check(&mut deck);
    assert!(deck.slide(0).unwrap().text().ends_with("beta\t20"));
}

#[test]
fn speaker_notes_create_the_notes_master_once() {
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Title)
        .unwrap()
        .set_notes("First notes\nsecond line")
        .unwrap();
    deck.add_slide(LayoutKind::Blank).unwrap();
    deck.add_slide(LayoutKind::Blank)
        .unwrap()
        .set_notes("Third")
        .unwrap();
    let mut deck = save_and_check(&mut deck);
    assert_eq!(
        deck.slide(0).unwrap().notes_text().as_deref(),
        Some("First notes\nsecond line")
    );
    assert_eq!(deck.slide(1).unwrap().notes_text(), None);
    assert_eq!(deck.slide(2).unwrap().notes_text().as_deref(), Some("Third"));
    let masters: Vec<_> = deck
        .package()
        .parts()
        .filter(|(n, _)| n.as_str().starts_with("/ppt/notesMasters/"))
        .collect();
    assert_eq!(masters.len(), 1);
    assert!(deck.presentation().notes_master_id_lst.is_some());
    let notes_part = deck.slide(0).unwrap().notes_part_name().unwrap().clone();
    assert_eq!(
        deck.package()
            .related_part(Some(&notes_part), rel_types::SLIDE)
            .as_ref(),
        Some(deck.slide(0).unwrap().part_name())
    );
    // Replacing existing notes keeps the same part.
    deck.slide_mut(0).unwrap().set_notes("Updated").unwrap();
    let deck = save_and_check(&mut deck);
    assert_eq!(deck.slide(0).unwrap().notes_text().as_deref(), Some("Updated"));
    assert_eq!(deck.slide(0).unwrap().notes_part_name(), Some(&notes_part));
}

#[test]
fn backgrounds_and_hidden_slides() {
    let mut deck = Presentation::new();
    {
        let mut s = deck.add_slide(LayoutKind::Blank).unwrap();
        assert_eq!(s.background_color(), None);
        s.set_background_color(Rgb(0x10, 0x20, 0x30));
        s.set_hidden(true);
    }
    deck.add_slide(LayoutKind::Blank).unwrap();
    let mut deck = save_and_check(&mut deck);
    assert_eq!(
        deck.slide(0).unwrap().background_color(),
        Some(Rgb(0x10, 0x20, 0x30))
    );
    assert!(deck.slide(0).unwrap().is_hidden());
    assert!(!deck.slide(1).unwrap().is_hidden());
    deck.slide_mut(0).unwrap().set_hidden(false);
    let deck = save_and_check(&mut deck);
    assert!(!deck.slide(0).unwrap().is_hidden());
}

#[test]
fn replacing_text_in_shapes_and_tables() {
    let mut deck = Presentation::new();
    {
        let mut s = deck.add_slide(LayoutKind::TitleAndContent).unwrap();
        s.set_title("Hello {name}").unwrap();
        s.set_body_text(&["{name} and {name}"]).unwrap();
        let mut t = s
            .add_table(1, 1, Length::ZERO, Length::ZERO, Length::cm(2.0), Length::cm(1.0))
            .unwrap();
        t.set_cell_text(0, 0, "cell {name}").unwrap();
        assert_eq!(s.replace_text("{name}", "Ada"), 4);
        assert_eq!(s.replace_text("{missing}", "x"), 0);
    }
    let deck = save_and_check(&mut deck);
    assert_eq!(deck.slide(0).unwrap().text(), "Hello Ada\nAda and Ada\ncell Ada");
}

#[test]
fn removing_slides_removes_their_parts() {
    let png = tiny_png(4, 4);
    let mut deck = Presentation::new();
    for i in 0..3 {
        let mut s = deck.add_slide(LayoutKind::TitleOnly).unwrap();
        s.set_title(&format!("Slide {i}")).unwrap();
        s.set_notes(&format!("notes {i}")).unwrap();
    }
    deck.slide_mut(1)
        .unwrap()
        .add_picture(&png, Length::ZERO, Length::ZERO, Length::cm(1.0), None)
        .unwrap();
    let mut deck = save_and_check(&mut deck);
    let removed = deck.slide(1).unwrap().part_name().clone();
    let removed_notes = deck.slide(1).unwrap().notes_part_name().unwrap().clone();
    deck.remove_slide(1).unwrap();
    assert!(matches!(deck.remove_slide(10), Err(Error::NotFound(_))));
    let deck = save_and_check(&mut deck);
    assert_eq!(deck.slide_count(), 2);
    let titles: Vec<_> = deck.slides().iter().map(|s| s.title().unwrap()).collect();
    assert_eq!(titles, ["Slide 0", "Slide 2"]);
    assert!(!deck.package().contains(&removed));
    assert!(!deck.package().contains(&removed_notes));
    assert!(
        !deck
            .package()
            .parts()
            .any(|(n, _)| n.as_str().starts_with("/ppt/media/")),
        "the picture only used by the removed slide is gone"
    );
    assert!(
        deck.package()
            .parts()
            .any(|(n, _)| n.as_str().starts_with("/ppt/notesMasters/")),
        "shared parts stay"
    );
    assert_eq!(deck.layouts().len(), 6);
    // New slides reuse the free part name but get a fresh id.
    let mut deck = deck;
    let s = deck.add_slide(LayoutKind::Blank).unwrap();
    assert_eq!(s.part_name().as_str(), "/ppt/slides/slide2.xml");
    assert_eq!(s.id(), 259);
}

#[test]
fn removing_every_slide_leaves_a_valid_presentation() {
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Blank).unwrap();
    deck.remove_slide(0).unwrap();
    let deck = save_and_check(&mut deck);
    assert_eq!(deck.slide_count(), 0);
    assert!(deck.presentation().sld_id_lst.is_none());
}

#[test]
fn moving_slides() {
    let mut deck = Presentation::new();
    for t in ["A", "B", "C", "D"] {
        deck.add_slide(LayoutKind::TitleOnly)
            .unwrap()
            .set_title(t)
            .unwrap();
    }
    deck.move_slide(0, 3).unwrap();
    deck.move_slide(2, 0).unwrap();
    assert!(deck.move_slide(0, 4).is_err());
    let deck = save_and_check(&mut deck);
    let titles: Vec<_> = deck.slides().iter().map(|s| s.title().unwrap()).collect();
    assert_eq!(titles, ["D", "B", "C", "A"]);
}

#[test]
fn slide_sizes() {
    let mut deck = Presentation::new();
    deck.set_slide_size(Length::emu(9_144_000), Length::emu(6_858_000))
        .unwrap();
    assert_eq!(
        deck.presentation().sld_sz.as_ref().unwrap().type_,
        Some(pml::ST_SlideSizeType::Screen4x3)
    );
    deck.set_slide_size(Length::cm(20.0), Length::cm(10.0)).unwrap();
    assert!(deck.set_slide_size(Length::cm(1.0), Length::cm(10.0)).is_err());
    assert!(
        deck.set_slide_size(Length::inches(60.0), Length::cm(10.0))
            .is_err()
    );
    let deck = save_and_check(&mut deck);
    assert_eq!(deck.slide_size(), (Length::cm(20.0), Length::cm(10.0)));
    assert_eq!(
        deck.presentation().sld_sz.as_ref().unwrap().type_,
        Some(pml::ST_SlideSizeType::Custom)
    );
}

#[test]
fn layout_selection() {
    let mut deck = Presentation::new();
    assert_eq!(
        deck.add_slide("title only").unwrap().layout_name(),
        Some("Title Only")
    );
    assert_eq!(
        deck.add_slide(LayoutRef::Index(0)).unwrap().layout_name(),
        Some("Title Slide")
    );
    assert_eq!(
        deck.add_slide(LayoutKind::TwoContent).unwrap().layout_name(),
        Some("Two Content")
    );
    assert!(matches!(
        deck.add_slide("No such layout"),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        deck.add_slide(LayoutKind::Comparison),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(deck.add_slide(99usize), Err(Error::NotFound(_))));
    assert_eq!(deck.slide_count(), 3);
}

#[test]
fn raw_access_and_core_properties() {
    let mut deck = Presentation::new();
    deck.presentation_mut().first_slide_num = Some(5);
    {
        let mut s = deck.add_slide(LayoutKind::Blank).unwrap();
        s.raw_mut().show_master_sp = Some(false);
    }
    let mut props = deck.core_properties().unwrap();
    props.title = Some("Deck title".into());
    props.creator = Some("Tester".into());
    deck.set_core_properties(&props).unwrap();
    let deck = save_and_check(&mut deck);
    assert_eq!(deck.presentation().first_slide_num, Some(5));
    assert_eq!(deck.slide(0).unwrap().raw().show_master_sp, Some(false));
    let props = deck.core_properties().unwrap();
    assert_eq!(props.title.as_deref(), Some("Deck title"));
    assert_eq!(props.creator.as_deref(), Some("Tester"));
}

#[test]
fn files_and_writers() {
    let dir = std::env::temp_dir().join(format!("openxml-pptx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("deck.pptx");
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Title)
        .unwrap()
        .set_title("On disk")
        .unwrap();
    deck.save(&path).unwrap();
    let reopened = Presentation::open(&path).unwrap();
    assert_eq!(reopened.slide(0).unwrap().title().as_deref(), Some("On disk"));
    let cursor = deck.write_to(std::io::Cursor::new(Vec::new())).unwrap();
    let from_reader = Presentation::from_reader(std::io::Cursor::new(cursor.into_inner())).unwrap();
    assert_eq!(from_reader.slide_count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(Presentation::open(dir.join("missing.pptx")).is_err());
}

#[test]
fn opening_something_else_fails_clearly() {
    let mut pkg = Package::new();
    let doc = PartName::new("/word/document.xml").unwrap();
    pkg.add_part(
        doc.clone(),
        openxml_opc::known::content_types::WML_DOCUMENT,
        b"<w:document/>".to_vec(),
    )
    .unwrap();
    pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &doc)
        .unwrap();
    let bytes = pkg.to_bytes().unwrap();
    assert!(matches!(
        Presentation::from_bytes(&bytes),
        Err(Error::InvalidDocument(_))
    ));
    let empty = Package::new().to_bytes().unwrap();
    assert!(matches!(
        Presentation::from_bytes(&empty),
        Err(Error::InvalidDocument(_))
    ));
    assert!(Presentation::from_bytes(b"garbage").is_err());
}

#[test]
fn text_of_the_whole_presentation() {
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::TitleOnly)
        .unwrap()
        .set_title("One")
        .unwrap();
    deck.add_slide(LayoutKind::TitleOnly)
        .unwrap()
        .set_title("Two")
        .unwrap();
    assert_eq!(deck.text(), "One\n\nTwo");
    assert_eq!(Presentation::default().slide_count(), 0);
}
