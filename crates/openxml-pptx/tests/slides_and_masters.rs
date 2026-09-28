//! Comments, slide operations (duplicate, import, hide, notes), themes,
//! layouts, footers, tables and presentation-level settings:
//! create → save → reopen → assert, with XSD and validator checks.

mod common;

use openxml_core::image::tiny_png;
use openxml_opc::PartName;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_pptx::{
    Alignment, Animation, Bullet, CellBorder, Color, DateField, Effect, Fill, FontSet, Gradient,
    HeaderFooter, LayoutKind, Length, Line, Link, PlaceholderKind, Presentation, PropertyValue, Rgb,
    SchemeColor, ShapeKind, ShapeType, ShowSettings, ShowSlides, ShowType, SlideSize, TableFlags, TableStyle,
    TextAnchor, ThemeFonts, Transition, TransitionEffect,
};

fn cm(v: f64) -> Length {
    Length::cm(v)
}

#[test]
fn legacy_comments_add_list_remove() {
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Blank).unwrap();
    deck.add_slide(LayoutKind::Blank).unwrap();
    let mut slide = deck.slide_mut(0).unwrap();
    let a1 = slide
        .add_comment_dated(
            "Ada Lovelace",
            "AL",
            "First",
            Length::pt(36.0),
            Length::pt(72.5),
            "2024-05-01T10:00:00Z",
        )
        .unwrap();
    let a2 = slide
        .add_comment("Ada Lovelace", "AL", "Second", cm(3.0), cm(4.0))
        .unwrap();
    let b1 = slide
        .add_comment("Grace Hopper", "GH", "Other author", cm(5.0), cm(1.0))
        .unwrap();
    assert_eq!((a1, a2, b1), (1, 2, 1));
    deck.slide_mut(1)
        .unwrap()
        .add_comment("Grace Hopper", "GH", "On slide 2", cm(0.0), cm(0.0))
        .unwrap();

    let mut deck = common::round_trip(&mut deck);
    let authors = deck.comment_authors().unwrap();
    assert_eq!(authors.len(), 2);
    assert_eq!(
        (authors[0].name.as_str(), authors[0].initials.as_str()),
        ("Ada Lovelace", "AL")
    );
    let comments = deck.comments(0).unwrap();
    assert_eq!(comments.len(), 3);
    assert_eq!(comments[0].text, "First");
    assert_eq!(comments[0].date, "2024-05-01T10:00:00Z");
    assert_eq!(
        comments[0].position,
        (Length::pt(36.0), Length::pt(72.5)),
        "1/8 pt resolution"
    );
    assert_eq!(comments[2].author, "Grace Hopper");
    assert_eq!(deck.comments(1).unwrap()[0].index, 2, "indexes count per author");
    let pkg = deck.package();
    assert_eq!(
        pkg.part(&PartName::new("/ppt/commentAuthors.xml").unwrap())
            .unwrap()
            .content_type(),
        ct::PML_COMMENT_AUTHORS
    );
    let slide_part = deck.slide(0).unwrap().part_name().clone();
    let comments_part = pkg.related_part(Some(&slide_part), rel_types::COMMENTS).unwrap();
    assert_eq!(pkg.part(&comments_part).unwrap().content_type(), ct::PML_COMMENTS);

    let mut slide = deck.slide_mut(0).unwrap();
    assert!(slide.remove_comment(0, 2).unwrap());
    assert!(!slide.remove_comment(0, 2).unwrap());
    let mut deck = common::round_trip(&mut deck);
    assert_eq!(deck.comments(0).unwrap().len(), 2);
    let mut slide = deck.slide_mut(0).unwrap();
    slide.remove_comment(0, 1).unwrap();
    slide.remove_comment(1, 1).unwrap();
    let deck = common::round_trip(&mut deck);
    assert!(deck.comments(0).unwrap().is_empty());
    assert!(
        deck.package()
            .related_part(Some(&slide_part), rel_types::COMMENTS)
            .is_none(),
        "the empty comments part is removed"
    );
    assert_eq!(deck.comments(1).unwrap().len(), 1);
    assert!(deck.comments(5).is_err());
}

#[test]
fn modern_comments_are_preserved() {
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Blank).unwrap();
    deck.flush().unwrap();
    let slide_part = deck.slide(0).unwrap().part_name().clone();
    let modern = PartName::new("/ppt/comments/modernComment_100_0.xml").unwrap();
    let authors = PartName::new("/ppt/authors.xml").unwrap();
    let xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p188:cmLst xmlns:p188="http://schemas.microsoft.com/office/powerpoint/2018/8/main"/>"#;
    let pkg = deck.package_mut();
    pkg.add_part(
        modern.clone(),
        "application/vnd.ms-powerpoint.comments+xml",
        xml.to_vec(),
    )
    .unwrap();
    pkg.add_part(
        authors.clone(),
        "application/vnd.ms-powerpoint.authors+xml",
        xml.to_vec(),
    )
    .unwrap();
    pkg.add_relationship(
        Some(&slide_part),
        "http://schemas.microsoft.com/office/2018/10/relationships/comments",
        &modern,
    )
    .unwrap();
    let pres = deck.part_name().clone();
    deck.package_mut()
        .add_relationship(
            Some(&pres),
            "http://schemas.microsoft.com/office/2018/10/relationships/authors",
            &authors,
        )
        .unwrap();
    let bytes = deck.package().to_bytes().unwrap();
    let mut deck = Presentation::from_bytes(&bytes).unwrap();
    deck.slide_mut(0)
        .unwrap()
        .add_comment("Ada", "A", "legacy", cm(1.0), cm(1.0))
        .unwrap();
    deck.slide_mut(0).unwrap().set_title("x").ok();
    let deck = common::round_trip(&mut deck);
    assert_eq!(deck.package().part(&modern).unwrap().data(), xml);
    assert_eq!(deck.package().part(&authors).unwrap().data(), xml);
    assert_eq!(deck.comments(0).unwrap().len(), 1);
}

#[test]
fn duplicating_slides_copies_notes_comments_and_keeps_media_shared() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::TitleAndContent).unwrap();
    slide.set_title("Original").unwrap();
    slide
        .add_picture(&tiny_png(3, 3), cm(1.0), cm(1.0), cm(2.0), None)
        .unwrap();
    let shape = slide
        .add_shape(ShapeType::Rect, cm(5.0), cm(5.0), cm(2.0), cm(2.0))
        .id();
    slide
        .set_link(shape, Some(Link::Url("https://example.com".into())))
        .unwrap();
    slide
        .add_animation(shape, Animation::new(Effect::FadeIn))
        .unwrap();
    slide.set_transition(Some(Transition::new(TransitionEffect::Dissolve)));
    slide.set_notes("Speaker notes").unwrap();
    slide
        .add_comment("Ada", "A", "Look here", cm(1.0), cm(1.0))
        .unwrap();
    deck.add_slide(LayoutKind::Blank).unwrap();
    let copy = deck.duplicate_slide(0).unwrap();
    assert_eq!(copy, 1);
    deck.slide_mut(1).unwrap().set_title("Copy").unwrap();
    assert!(deck.duplicate_slide(9).is_err());

    let deck = common::round_trip(&mut deck);
    assert_eq!(deck.slide_count(), 3);
    let (a, b) = (deck.slide(0).unwrap(), deck.slide(1).unwrap());
    assert_eq!(a.title().as_deref(), Some("Original"));
    assert_eq!(b.title().as_deref(), Some("Copy"));
    assert_eq!(b.notes_text().as_deref(), Some("Speaker notes"));
    assert_ne!(a.notes_part_name(), b.notes_part_name());
    assert_eq!(b.transition().unwrap().effect, Some(TransitionEffect::Dissolve));
    assert_eq!(b.animations().len(), 1);
    assert_eq!(
        deck.slide_links(1)[0].link,
        Link::Url("https://example.com".into())
    );
    assert_eq!(deck.comments(1).unwrap()[0].text, "Look here");
    assert_eq!(deck.slide(2).unwrap().layout_name(), Some("Blank"));
    let images = deck
        .package()
        .parts()
        .filter(|(n, _)| n.as_str().starts_with("/ppt/media/"))
        .count();
    assert_eq!(images, 1, "the picture is shared");
    // The notes page of the copy points back at the copy.
    let notes = b.notes_part_name().unwrap();
    assert_eq!(
        deck.package()
            .related_part(Some(notes), rel_types::SLIDE)
            .as_ref(),
        Some(b.part_name())
    );
    let ids: Vec<u32> = deck
        .presentation()
        .sld_id_lst
        .as_ref()
        .unwrap()
        .sld_id
        .iter()
        .map(|e| e.id.unwrap())
        .collect();
    assert_eq!(
        ids,
        deck.slides().iter().map(|s| s.id()).collect::<Vec<_>>(),
        "id list order"
    );
}

#[test]
fn slides_are_imported_from_other_presentations() {
    let mut source = Presentation::new();
    {
        let mut s = source.add_slide(LayoutKind::TitleAndContent).unwrap();
        s.set_title("Imported").unwrap();
        s.set_body_text(&["one", "two"]).unwrap();
        s.add_picture(&tiny_png(5, 5), cm(1.0), cm(1.0), cm(2.0), None)
            .unwrap();
        s.set_notes("From the other deck").unwrap();
        s.add_comment("Linus", "LT", "imported comment", cm(2.0), cm(2.0))
            .unwrap();
        let t = s.add_text_box(cm(1.0), cm(8.0), cm(5.0), cm(1.0), "").id();
        s.add_link_run(t, "back", Link::Slide(0)).unwrap();
    }
    source.add_slide(LayoutKind::Blank).unwrap();
    let source = common::round_trip(&mut source);

    let mut target = Presentation::new();
    target.add_slide(LayoutKind::Title).unwrap();
    target
        .slide_mut(0)
        .unwrap()
        .add_picture(&tiny_png(5, 5), cm(1.0), cm(1.0), cm(2.0), None)
        .unwrap();
    let at = target.import_slide(&source, 0).unwrap();
    assert_eq!(at, 1);
    let at = target.import_slide(&source, 1).unwrap();
    assert_eq!(at, 2);
    assert!(target.import_slide(&source, 7).is_err());

    let target = common::round_trip(&mut target);
    let imported = target.slide(1).unwrap();
    assert_eq!(imported.title().as_deref(), Some("Imported"));
    assert!(imported.text().contains("one\ntwo"));
    assert_eq!(imported.layout_name(), Some("Title and Content"));
    assert_eq!(imported.notes_text().as_deref(), Some("From the other deck"));
    assert_eq!(target.comments(1).unwrap()[0].author, "Linus");
    assert_eq!(
        target.slide_links(1)[0].link,
        Link::Slide(1),
        "jumps point at the copy itself"
    );
    assert_eq!(target.slide(2).unwrap().layout_name(), Some("Blank"));
    let images = target
        .package()
        .parts()
        .filter(|(n, _)| n.as_str().starts_with("/ppt/media/"))
        .count();
    assert_eq!(images, 1, "identical images are stored once");
    // The notes use the target's notes master.
    let notes = imported.notes_part_name().unwrap();
    let master = target
        .package()
        .related_part(Some(notes), rel_types::NOTES_MASTER)
        .unwrap();
    assert!(target.package().contains(&master));
}

#[test]
fn hidden_slides_and_formatted_notes() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
    slide.set_hidden(true);
    {
        let mut notes = slide.notes_mut().unwrap();
        notes.add_paragraph("Agenda").set_bullet(Bullet::None);
        let mut p = notes.add_paragraph("");
        p.set_bullet(Bullet::numbered());
        p.add_run("Budget ").bold(true);
        p.add_run("in red").color(Rgb(0xC0, 0, 0));
    }
    let mut deck = common::round_trip(&mut deck);
    let slide = deck.slide(0).unwrap();
    assert!(slide.is_hidden());
    assert_eq!(slide.notes_text().as_deref(), Some("Agenda\nBudget in red"));
    let mut slide = deck.slide_mut(0).unwrap();
    let mut notes = slide.notes_mut().unwrap();
    let mut p = notes.paragraph_mut(1).unwrap();
    assert_eq!(p.bullet(), Some(Bullet::numbered()));
    assert!(p.run_mut(0).unwrap().is_bold());
    slide.set_hidden(false);
    let deck = common::round_trip(&mut deck);
    assert!(!deck.slide(0).unwrap().is_hidden());
}

#[test]
fn themes_backgrounds_layouts_and_footers() {
    let mut deck = Presentation::new();
    let mut colors = deck.theme_colors().unwrap();
    colors.accent1 = Rgb(0x12, 0x34, 0x56);
    colors.hyperlink = Rgb(0xAA, 0, 0xAA);
    deck.set_theme_colors(&colors, Some("Custom Colors")).unwrap();
    let fonts = ThemeFonts {
        major: FontSet {
            latin: "Georgia".into(),
            east_asian: "MS Mincho".into(),
            complex_script: "Times New Roman".into(),
        },
        minor: FontSet {
            latin: "Verdana".into(),
            east_asian: "".into(),
            complex_script: "".into(),
        },
    };
    deck.set_theme_fonts(&fonts, Some("Custom Fonts")).unwrap();
    deck.set_master_background(Fill::Gradient(Gradient::linear(
        Rgb::WHITE,
        SchemeColor::Accent1,
        90.0,
    )))
    .unwrap();
    {
        let mut layout = deck
            .add_layout("Photo with Caption", LayoutKind::TitleOnly)
            .unwrap();
        layout.add_placeholder(PlaceholderKind::Picture, cm(2.0), cm(4.0), cm(20.0), cm(10.0));
        layout.add_placeholder(PlaceholderKind::Body, cm(2.0), cm(15.0), cm(20.0), cm(2.0));
        layout.set_background(Fill::solid(Rgb(0xF0, 0xF0, 0xF0)));
    }
    assert!(deck.add_layout("x", "no such layout").is_err());
    deck.add_slide(LayoutKind::Title)
        .unwrap()
        .set_title("Cover")
        .unwrap();
    {
        let mut slide = deck.add_slide("Photo with Caption").unwrap();
        slide.set_title("Photo").unwrap();
        let id = slide.fill_picture_placeholder(&tiny_png(40, 10)).unwrap();
        let crop = slide.picture_mut(id).unwrap().crop();
        assert!(
            crop.left > 0.0,
            "a wide image is cropped left and right: {crop:?}"
        );
        slide.set_body_text(&["Caption"]).unwrap();
        slide.set_background(Fill::solid(SchemeColor::Background2));
    }
    deck.add_slide(LayoutKind::TitleAndContent).unwrap();
    deck.apply_header_footer(&HeaderFooter {
        footer: Some("ACME".into()),
        slide_number: true,
        date: Some(DateField::Automatic),
        skip_title_slides: true,
    })
    .unwrap();
    deck.slide_mut(2)
        .unwrap()
        .set_date(Some(DateField::Fixed("Q3 2024".into())))
        .unwrap();

    let mut deck = common::round_trip(&mut deck);
    let colors2 = deck.theme_colors().unwrap();
    assert_eq!(colors2, colors);
    assert_eq!(deck.theme_fonts().unwrap(), fonts);
    assert!(matches!(
        deck.master_background().unwrap(),
        Some(Fill::Gradient(_))
    ));
    assert_eq!(deck.master_header_footer().unwrap(), (true, true, true));
    let layout = deck
        .layouts()
        .iter()
        .find(|l| l.name() == "Photo with Caption")
        .unwrap();
    assert_eq!(layout.kind(), None);
    let kinds: Vec<PlaceholderKind> = layout.placeholders().iter().map(|p| p.kind).collect();
    assert!(kinds.contains(&PlaceholderKind::Picture) && kinds.contains(&PlaceholderKind::Body));
    let photo = deck.slide(1).unwrap();
    assert!(photo.shapes().iter().any(|s| s.kind == ShapeKind::Picture));
    assert_eq!(
        photo.background_fill(),
        Some(Fill::Solid(Color::Scheme(SchemeColor::Background2)))
    );
    assert_eq!(
        deck.slide(0).unwrap().footer_text(),
        None,
        "title slides are skipped"
    );
    assert!(!deck.slide(0).unwrap().has_slide_number());
    assert_eq!(photo.footer_text().as_deref(), Some("ACME"));
    assert!(photo.has_slide_number());
    assert!(photo.date_text().is_some_and(|d| d.contains('/')));
    assert_eq!(deck.slide(2).unwrap().date_text().as_deref(), Some("Q3 2024"));

    // Turning footers off removes the placeholders and records it in p:hf.
    deck.apply_header_footer(&HeaderFooter::default()).unwrap();
    let deck = common::round_trip(&mut deck);
    assert_eq!(deck.slide(1).unwrap().footer_text(), None);
    assert!(!deck.slide(1).unwrap().has_slide_number());
    assert_eq!(deck.master_header_footer().unwrap(), (false, false, false));
}

#[test]
fn tables_with_merges_styles_and_cell_formatting() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::TitleOnly).unwrap();
    let id = {
        let mut t = slide
            .add_table(3, 3, cm(2.0), cm(4.0), cm(18.0), cm(6.0))
            .unwrap();
        t.set_values([["Region", "Q1", "Q2"], ["North", "10", "12"], ["Total", "", ""]])
            .unwrap();
        t.merge_cells(2, 1, 2, 2).unwrap();
        t.set_cell_fill(0, 0, Fill::solid(Rgb(0x20, 0x40, 0x80))).unwrap();
        t.set_cell_borders(1, 1, Line::solid(Rgb::BLACK, Length::pt(1.5)))
            .unwrap();
        t.set_cell_border(
            1,
            2,
            CellBorder::DiagonalUp,
            Line::solid(Rgb(0xFF, 0, 0), Length::pt(1.0)),
        )
        .unwrap();
        t.set_cell_margins(1, 0, cm(0.5), cm(0.1), cm(0.5), cm(0.1))
            .unwrap();
        t.set_cell_anchor(0, 1, TextAnchor::Middle).unwrap();
        t.set_style(TableStyle::LightStyle2Accent1);
        t.set_flags(TableFlags {
            first_row: true,
            last_row: true,
            banded_rows: true,
            ..TableFlags::default()
        });
        t.set_row_height(0, cm(1.5)).unwrap();
        t.set_column_width(0, cm(8.0)).unwrap();
        t.insert_row(2).unwrap();
        t.set_cell_text(2, 0, "South").unwrap();
        t.insert_column(3).unwrap();
        t.remove_column(3).unwrap();
        t.with_cell_paragraph(0, 0, 0, |mut p| {
            p.set_alignment(Alignment::Center);
        })
        .unwrap();
        t.set_alt_text(Some("Sales"), "Quarterly sales by region");
        t.id()
    };
    slide.add_animation(id, Animation::new(Effect::FadeIn)).unwrap();

    let mut deck = common::round_trip(&mut deck);
    let slide = deck.slide(0).unwrap();
    assert!(
        slide
            .text()
            .contains("Region\tQ1\tQ2\nNorth\t10\t12\nSouth\t\t\nTotal\t\t")
    );
    let mut slide = deck.slide_mut(0).unwrap();
    let t = slide.table_mut(id).unwrap();
    assert_eq!((t.rows(), t.cols()), (4, 3));
    assert_eq!(t.cell_span(3, 1), Some((1, 2, false)));
    assert_eq!(t.cell_span(3, 2), Some((1, 1, true)));
    assert!(matches!(t.cell_fill(0, 0), Some(Fill::Solid(_))));
    assert_eq!(
        t.cell_border(1, 1, CellBorder::Bottom).unwrap().width,
        Length::pt(1.5)
    );
    assert!(t.cell_border(1, 2, CellBorder::DiagonalUp).is_some());
    assert_eq!(t.cell_margins(1, 0).unwrap().0, cm(0.5));
    assert_eq!(t.cell_anchor(0, 1), Some(TextAnchor::Middle));
    assert_eq!(t.style(), Some(TableStyle::LightStyle2Accent1));
    assert!(t.flags().last_row && t.flags().banded_rows && !t.flags().first_column);
    assert_eq!(t.row_height(0), Some(cm(1.5)));
    assert_eq!(t.column_width(0), Some(cm(8.0)));
    let info = slide.shapes().into_iter().find(|s| s.id == id).unwrap();
    // 8 + 6 + 6 cm wide; 1.5 + 3 × 2 cm high.
    assert_eq!(info.size, Some((cm(20.0), cm(7.5))));
}

#[test]
fn presentation_level_settings() {
    let mut deck = Presentation::new();
    for i in 0..4 {
        deck.add_slide(LayoutKind::TitleOnly)
            .unwrap()
            .set_title(&format!("S{i}"))
            .unwrap();
    }
    deck.set_slide_size_preset(SlideSize::A4).unwrap();
    let short = deck.add_custom_show("Short", &[0, 2]).unwrap();
    let reversed = deck.add_custom_show("Reversed", &[3, 2, 1, 0]).unwrap();
    assert!(deck.add_custom_show("Bad", &[9]).is_err());
    deck.set_show_settings(ShowSettings {
        show_type: ShowType::Kiosk {
            restart_after_ms: Some(120_000),
        },
        slides: ShowSlides::CustomShow(short),
        ..ShowSettings::default()
    })
    .unwrap();
    assert!(
        deck.set_show_settings(ShowSettings {
            slides: ShowSlides::CustomShow(99),
            ..ShowSettings::default()
        })
        .is_err()
    );
    deck.set_custom_property("Project", PropertyValue::Text("Aurora".into()))
        .unwrap();
    deck.set_custom_property("Revision", PropertyValue::Integer(7))
        .unwrap();
    deck.set_custom_property("Approved", PropertyValue::Bool(true))
        .unwrap();
    deck.set_custom_property("Budget", PropertyValue::Number(1.5e6))
        .unwrap();
    deck.set_custom_property("Due", PropertyValue::DateTime("2024-12-31T00:00:00Z".into()))
        .unwrap();
    deck.set_custom_property("Revision", PropertyValue::Integer(8))
        .unwrap();
    assert!(deck.set_custom_property("", PropertyValue::Bool(true)).is_err());
    let mut core = deck.core_properties().unwrap();
    core.title = Some("Aurora kick-off".into());
    deck.set_core_properties(&core).unwrap();

    let mut deck = common::round_trip(&mut deck);
    assert_eq!(deck.slide_size_preset(), SlideSize::A4);
    assert_eq!(deck.slide_size(), SlideSize::A4.dimensions());
    let shows = deck.custom_shows();
    assert_eq!(shows.len(), 2);
    assert_eq!(
        (shows[0].id, shows[0].name.as_str(), shows[0].slides.clone()),
        (short, "Short", vec![0, 2])
    );
    assert_eq!(shows[1].slides, vec![3, 2, 1, 0]);
    let settings = deck.show_settings().unwrap();
    assert_eq!(
        settings.show_type,
        ShowType::Kiosk {
            restart_after_ms: Some(120_000)
        }
    );
    assert!(settings.loop_until_esc);
    assert_eq!(settings.slides, ShowSlides::CustomShow(short));
    let props = deck.custom_properties().unwrap();
    assert_eq!(props.len(), 5);
    assert!(props.contains(&("Revision".into(), PropertyValue::Integer(8))));
    assert!(props.contains(&("Budget".into(), PropertyValue::Number(1.5e6))));
    assert_eq!(
        deck.core_properties().unwrap().title.as_deref(),
        Some("Aurora kick-off")
    );
    let custom = PartName::new("/docProps/custom.xml").unwrap();
    assert_eq!(
        deck.package().part(&custom).unwrap().content_type(),
        ct::CUSTOM_PROPERTIES
    );

    // Removing a slide removes it from custom shows.
    deck.remove_slide(2).unwrap();
    assert!(deck.remove_custom_show(reversed));
    assert!(!deck.remove_custom_show(reversed));
    assert!(deck.remove_custom_property("Approved").unwrap());
    assert!(!deck.remove_custom_property("Approved").unwrap());
    deck.set_slide_size_preset(SlideSize::Custom(cm(20.0), cm(20.0)))
        .unwrap();
    let deck = common::round_trip(&mut deck);
    assert_eq!(deck.custom_shows()[0].slides, vec![0]);
    assert_eq!(deck.custom_properties().unwrap().len(), 4);
    assert_eq!(deck.slide_size_preset(), SlideSize::Custom(cm(20.0), cm(20.0)));
}

fn inject_sections(deck: &mut Presentation, sections: &[(&str, &[usize])]) {
    let ids: Vec<u32> = deck.slides().iter().map(|s| s.id()).collect();
    let mut xml = String::from(
        r#"<p14:sectionLst xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main">"#,
    );
    for (i, (name, slides)) in sections.iter().enumerate() {
        xml.push_str(&format!(
            r#"<p14:section name="{name}" id="{{00000000-0000-0000-0000-00000000000{i}}}"><p14:sldIdLst>"#
        ));
        for s in *slides {
            xml.push_str(&format!(r#"<p14:sldId id="{}"/>"#, ids[*s]));
        }
        xml.push_str("</p14:sldIdLst></p14:section>");
    }
    xml.push_str("</p14:sectionLst>");
    let raw = openxml_xml::RawElement::parse(&xml).unwrap();
    let ext_lst = deck.presentation_mut().ext_lst.get_or_insert_with(Box::default);
    ext_lst.ext.push(openxml_schema::pml::CT_Extension {
        uri: Some("{521415D9-36F7-43E2-AB2F-B90AF26B5E84}".into()),
        any: vec![raw],
        ..Default::default()
    });
}

#[test]
fn sections_follow_slide_operations() {
    let mut deck = Presentation::new();
    for i in 0..3 {
        deck.add_slide(LayoutKind::TitleOnly)
            .unwrap()
            .set_title(&format!("S{i}"))
            .unwrap();
    }
    inject_sections(&mut deck, &[("Intro", &[0]), ("Body", &[1, 2])]);
    let mut deck = common::round_trip(&mut deck);
    let names: Vec<(String, Vec<usize>)> = deck.sections().into_iter().map(|s| (s.name, s.slides)).collect();
    assert_eq!(
        names,
        vec![("Intro".into(), vec![0]), ("Body".into(), vec![1, 2])]
    );

    deck.duplicate_slide(0).unwrap(); // S0, S0', S1, S2
    deck.add_slide(LayoutKind::Blank).unwrap(); // appended to the last section
    let mut other = Presentation::new();
    other.add_slide(LayoutKind::Blank).unwrap();
    deck.import_slide(&other, 0).unwrap();
    let deck2 = common::round_trip(&mut deck);
    let sections = deck2.sections();
    assert_eq!(sections[0].slides, vec![0, 1]);
    assert_eq!(sections[1].slides, vec![2, 3, 4, 5]);

    deck.remove_slide(2).unwrap(); // S0, S0', S2, blank, imported
    deck.move_slide(3, 0).unwrap(); // blank, S0, S0', S2, imported
    let deck = common::round_trip(&mut deck);
    let sections = deck.sections();
    assert_eq!(sections[0].slides, vec![0, 1, 2]);
    assert_eq!(sections[1].slides, vec![3, 4]);
    let total: usize = sections.iter().map(|s| s.slides.len()).sum();
    assert_eq!(total, deck.slide_count(), "every slide is in exactly one section");
}

#[test]
fn application_properties() {
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Blank).unwrap();
    deck.edit_app_properties(|p| {
        p.company = Some("ACME".into());
        p.manager = Some("Grace".into());
    })
    .unwrap();
    let deck = common::round_trip(&mut deck);
    let app = deck.app_properties().unwrap().unwrap();
    assert_eq!(app.company.as_deref(), Some("ACME"));
    assert_eq!(app.manager.as_deref(), Some("Grace"));
    assert_eq!(app.slides, Some(1));
}
