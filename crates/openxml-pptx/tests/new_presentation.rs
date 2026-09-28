//! A new presentation and its template.

mod common;

use openxml_opc::known::{content_types as ct, rel_types};
use openxml_pptx::{LayoutKind, Length, PlaceholderKind, Presentation};

#[test]
fn empty_presentation_is_valid_and_complete() {
    let mut deck = Presentation::new();
    assert_eq!(deck.slide_count(), 0);
    assert_eq!(
        deck.slide_size(),
        (Length::emu(12_192_000), Length::emu(6_858_000))
    );
    let bytes = deck.to_bytes().unwrap();
    let deck = common::check_saved(&bytes);
    let pkg = deck.package();
    let pres = deck.part_name();
    assert_eq!(pkg.part(pres).unwrap().content_type(), ct::PML_PRESENTATION);
    for rel in [
        rel_types::SLIDE_MASTER,
        rel_types::THEME,
        rel_types::PRES_PROPS,
        rel_types::VIEW_PROPS,
        rel_types::TABLE_STYLES,
    ] {
        assert!(pkg.related_part(Some(pres), rel).is_some(), "missing {rel}");
    }
    assert!(pkg.related_part(None, rel_types::CORE_PROPERTIES).is_some());
    assert!(pkg.related_part(None, rel_types::EXTENDED_PROPERTIES).is_some());
    assert!(deck.presentation().notes_sz.is_some());
    assert!(deck.presentation().default_text_style.is_some());
    assert!(deck.presentation().sld_id_lst.is_none(), "no empty slide list");
    let core = deck.core_properties().unwrap();
    assert_eq!(core.creator.as_deref(), Some("openxml-rust"));
    assert!(core.created.is_some());
}

#[test]
fn template_offers_the_common_layouts() {
    let deck = Presentation::new();
    let names: Vec<&str> = deck.layouts().iter().map(|l| l.name()).collect();
    assert_eq!(
        names,
        [
            "Title Slide",
            "Title and Content",
            "Section Header",
            "Two Content",
            "Title Only",
            "Blank"
        ]
    );
    let kinds: Vec<_> = deck.layouts().iter().map(|l| l.kind()).collect();
    assert_eq!(
        kinds,
        [
            Some(LayoutKind::Title),
            Some(LayoutKind::TitleAndContent),
            Some(LayoutKind::SectionHeader),
            Some(LayoutKind::TwoContent),
            Some(LayoutKind::TitleOnly),
            Some(LayoutKind::Blank),
        ]
    );
    let title = &deck.layouts()[0];
    let phs: Vec<_> = title.placeholders().iter().map(|p| p.kind).collect();
    assert!(phs.contains(&PlaceholderKind::CenteredTitle));
    assert!(phs.contains(&PlaceholderKind::Subtitle));
    assert!(phs.contains(&PlaceholderKind::SlideNumber));
    for layout in deck.layouts() {
        assert_eq!(
            layout.master_part_name().as_str(),
            "/ppt/slideMasters/slideMaster1.xml"
        );
        assert!(layout.part_name().as_str().starts_with("/ppt/slideLayouts/"));
        assert!(layout.raw().c_sld.is_some());
    }
    for k in [LayoutKind::Title, LayoutKind::Blank] {
        assert_eq!(
            deck.layouts()
                .iter()
                .find(|l| l.kind() == Some(k))
                .unwrap()
                .name(),
            k.default_name()
        );
    }
}

#[test]
fn slides_from_every_layout_are_valid() {
    let mut deck = Presentation::new();
    for i in 0..deck.layouts().len() {
        deck.add_slide(i).unwrap();
    }
    let bytes = deck.to_bytes().unwrap();
    let deck = common::check_saved(&bytes);
    assert_eq!(deck.slide_count(), 6);
    let layouts: Vec<_> = deck
        .slides()
        .iter()
        .map(|s| s.layout_name().unwrap().to_owned())
        .collect();
    assert_eq!(
        layouts,
        [
            "Title Slide",
            "Title and Content",
            "Section Header",
            "Two Content",
            "Title Only",
            "Blank"
        ]
    );
    // Footer-area placeholders are not copied onto new slides.
    let title_slide = deck.slide(0).unwrap();
    let kinds: Vec<_> = title_slide
        .shapes()
        .iter()
        .filter_map(|s| s.placeholder.map(|p| p.kind))
        .collect();
    assert_eq!(kinds, [PlaceholderKind::CenteredTitle, PlaceholderKind::Subtitle]);
    let blank = deck.slide(5).unwrap();
    assert!(blank.shapes().is_empty());
    let two = deck.slide(3).unwrap();
    let indexes: Vec<_> = two
        .shapes()
        .iter()
        .filter_map(|s| s.placeholder.and_then(|p| p.index))
        .collect();
    assert_eq!(indexes, [1, 2]);
    // Slide identifiers are consecutive from 256.
    let ids: Vec<u32> = deck.slides().iter().map(|s| s.id()).collect();
    assert_eq!(ids, [256, 257, 258, 259, 260, 261]);
}

#[test]
fn app_properties_track_slide_counts() {
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Title).unwrap();
    deck.add_slide(LayoutKind::Blank).unwrap().set_notes("n").unwrap();
    deck.slide_mut(1).unwrap().set_hidden(true);
    let bytes = deck.to_bytes().unwrap();
    let deck = Presentation::from_bytes(&bytes).unwrap();
    let app = deck
        .package()
        .related_part(None, rel_types::EXTENDED_PROPERTIES)
        .unwrap();
    let props = openxml_schema::shared_extended_properties::elements::PROPERTIES
        .parse_bytes(deck.package().part(&app).unwrap().data())
        .unwrap();
    assert_eq!(props.slides, Some(2));
    assert_eq!(props.notes, Some(1));
    assert_eq!(props.hidden_slides, Some(1));
    assert_eq!(props.application.as_deref(), Some("openxml-rust"));
}
