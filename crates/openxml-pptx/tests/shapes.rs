//! Autoshapes, text formatting, connectors, groups and pictures:
//! create → save → reopen → assert, with XSD and validator checks.

mod common;

use openxml_core::image::tiny_png;
use openxml_pptx::{
    Alignment, ArrowHead, ArrowKind, ArrowSize, AutoNumberScheme, Autofit, Bullet, Color, ConnectorKind,
    Crop, Fill, FontSize, Freeform, Gradient, LayoutKind, Length, Line, LineDash, PatternType, Presentation,
    Rgb, SchemeColor, Shadow, ShapeKind, ShapeType, Side, Spacing, TextAnchor, TextDirection,
};
use openxml_schema::{dml, pml};

fn cm(v: f64) -> Length {
    Length::cm(v)
}

#[test]
fn autoshapes_with_fills_lines_effects_and_transforms() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
    let mut ids = Vec::new();
    for (i, geometry) in [
        ShapeType::Rect,
        ShapeType::RoundRect,
        ShapeType::Ellipse,
        ShapeType::Star5,
        ShapeType::RightArrow,
    ]
    .into_iter()
    .enumerate()
    {
        let x = cm(1.0 + 5.0 * i as f64);
        let shape = slide.add_shape(geometry, x, cm(1.0), cm(4.0), cm(3.0));
        ids.push(shape.id());
    }
    {
        let mut s = slide.shape_by_id(ids[0]).unwrap();
        s.set_fill(Fill::solid(Rgb(0x1F, 0x4E, 0x79)))
            .set_line(Line::solid(SchemeColor::Accent2, Length::pt(3.0)).dash(LineDash::DashDot))
            .set_shadow(Some(Shadow::default()))
            .set_rotation(30.0)
            .set_flip(true, false);
        s.set_text("Solid");
    }
    {
        let mut s = slide.shape_by_id(ids[1]).unwrap();
        s.set_fill(Fill::Gradient(Gradient {
            stops: vec![
                (0.0, Color::Rgb(Rgb(255, 0, 0))),
                (0.5, Color::Scheme(SchemeColor::Accent4)),
                (1.0, Color::Rgb(Rgb(0, 0, 255))),
            ],
            angle: 90.0,
        }))
        .set_adjust("adj", 30000)
        .set_line(Line::none());
    }
    {
        let mut s = slide.shape_by_id(ids[2]).unwrap();
        s.set_fill(Fill::Pattern {
            pattern: PatternType::DkDnDiag,
            foreground: Color::Rgb(Rgb::BLACK),
            background: Color::Scheme(SchemeColor::Background1),
        })
        .set_flip(false, true);
    }
    {
        let mut s = slide.shape_by_id(ids[3]).unwrap();
        s.set_fill(Fill::SolidAlpha(Color::Rgb(Rgb(0xFF, 0xC0, 0)), 0.5))
            .set_line(
                Line::solid(Rgb::BLACK, Length::pt(1.0))
                    .head(ArrowHead::new(ArrowKind::Oval))
                    .tail(ArrowHead {
                        kind: ArrowKind::Stealth,
                        width: ArrowSize::Large,
                        length: ArrowSize::Small,
                    }),
            );
    }
    let free = slide
        .add_shape(ShapeType::Rect, cm(1.0), cm(6.0), cm(3.0), cm(3.0))
        .id();
    slide.shape_by_id(free).unwrap().set_freeform(&Freeform::polygon(
        1000,
        1000,
        &[(0, 1000), (500, 0), (1000, 1000)],
    ));

    let deck = common::round_trip(&mut deck);
    let slide = deck.slide(0).unwrap();
    let shapes = slide.shapes();
    assert_eq!(shapes.len(), 6);
    assert!(shapes.iter().all(|s| s.kind == ShapeKind::AutoShape));
    assert_eq!(shapes[0].text, "Solid");
    assert_eq!(shapes[1].name, "Rectangle: Rounded Corners 2");

    let mut deck = deck;
    let mut slide = deck.slide_mut(0).unwrap();
    let s = slide.shape_by_id(ids[0]).unwrap();
    assert_eq!(
        s.fill_format(),
        Some(Fill::Solid(Color::Rgb(Rgb(0x1F, 0x4E, 0x79))))
    );
    let line = s.line_format().unwrap();
    assert_eq!(line.dash, Some(LineDash::DashDot));
    assert_eq!(line.color, Some(Color::Scheme(SchemeColor::Accent2)));
    assert_eq!(line.width, Length::pt(3.0));
    assert_eq!(s.shadow(), Some(Shadow::default()));
    assert_eq!(s.rotation(), 30.0);
    assert_eq!(s.flip(), (true, false));
    assert!(s.raw().style.is_some(), "autoshapes keep their theme style");
    let s = slide.shape_by_id(ids[1]).unwrap();
    let Some(Fill::Gradient(g)) = s.fill_format() else {
        panic!("gradient expected")
    };
    assert_eq!(g.stops.len(), 3);
    assert_eq!(g.angle, 90.0);
    assert_eq!(s.adjusts(), vec![("adj".to_owned(), 30000)]);
    assert_eq!(s.line_format().unwrap().color, None);
    let s = slide.shape_by_id(ids[2]).unwrap();
    assert!(matches!(
        s.fill_format(),
        Some(Fill::Pattern {
            pattern: PatternType::DkDnDiag,
            ..
        })
    ));
    assert_eq!(s.flip(), (false, true));
    let s = slide.shape_by_id(ids[3]).unwrap();
    assert!(matches!(s.fill_format(), Some(Fill::SolidAlpha(_, a)) if (a - 0.5).abs() < 1e-9));
    let line = s.line_format().unwrap();
    assert_eq!(line.head.unwrap().kind, ArrowKind::Oval);
    assert_eq!(line.tail.unwrap().width, ArrowSize::Large);
    let s = slide.shape_by_id(free).unwrap();
    assert!(matches!(
        s.raw().sp_pr.as_ref().unwrap().geometry,
        Some(dml::EG_Geometry::CustGeom(_))
    ));
}

#[test]
fn text_formatting_bullets_and_frames() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
    let id = {
        let mut tb = slide.add_text_box(cm(1.0), cm(1.0), cm(20.0), cm(10.0), "");
        tb.set_text_anchor(TextAnchor::Bottom)
            .set_text_insets(cm(0.5), cm(0.2), cm(0.5), cm(0.2))
            .set_autofit(Autofit::SHRINK)
            .set_columns(2, cm(1.0));
        {
            let mut p = tb.add_paragraph("Bulleted");
            p.set_bullet(Bullet::Char {
                char: '§',
                font: Some("Wingdings".into()),
            })
            .set_bullet_color(Rgb(0xC0, 0, 0))
            .set_indent(cm(1.0), cm(-0.5))
            .set_line_spacing(Spacing::Lines(1.2))
            .set_space_before(Spacing::Points(6.0))
            .set_space_after(Spacing::Points(3.0));
        }
        {
            let mut p = tb.add_paragraph("Numbered");
            p.set_bullet(Bullet::Numbered {
                scheme: AutoNumberScheme::RomanUcPeriod,
                start_at: 3,
            })
            .set_level(1)
            .set_alignment(Alignment::Right);
        }
        {
            let mut p = tb.add_paragraph("");
            p.add_run("Big ")
                .size(FontSize(40.0))
                .bold(true)
                .color(SchemeColor::Accent1);
            p.add_run("strike").strike(true).italic(true).underline(true);
            p.add_line_break();
            p.add_run("H").highlight(Rgb(0xFF, 0xFF, 0));
            p.add_run("2").baseline(-0.25);
            p.add_run("O").shadow(Some(Shadow::default()));
            p.add_run(" tiếng Việt 日本語 العربية")
                .font("Calibri")
                .east_asian_font("MS Mincho")
                .complex_script_font("Arial")
                .language("vi-VN")
                .character_spacing(1.5);
        }
        tb.id()
    };
    let vertical = {
        let mut s = slide.add_shape(ShapeType::Rect, cm(22.0), cm(1.0), cm(3.0), cm(10.0));
        s.set_text("Vertical");
        s.set_text_direction(TextDirection::Vertical270)
            .set_word_wrap(false)
            .set_autofit(Autofit::ResizeShape)
            .set_text_shadow(Some(Shadow::default()));
        s.id()
    };

    let mut deck = common::round_trip(&mut deck);
    let mut slide = deck.slide_mut(0).unwrap();
    let mut tb = slide.shape_by_id(id).unwrap();
    assert_eq!(
        tb.text(),
        "Bulleted\nNumbered\nBig strike\nH2O tiếng Việt 日本語 العربية"
    );
    assert_eq!(tb.text_anchor(), Some(TextAnchor::Bottom));
    assert_eq!(tb.text_insets(), (cm(0.5), cm(0.2), cm(0.5), cm(0.2)));
    assert_eq!(tb.autofit(), Some(Autofit::SHRINK));
    assert_eq!(tb.columns(), (2, cm(1.0)));
    {
        let p = tb.paragraph_mut(0).unwrap();
        assert_eq!(
            p.bullet(),
            Some(Bullet::Char {
                char: '§',
                font: Some("Wingdings".into())
            })
        );
        assert_eq!(p.indent(), Some((cm(1.0), cm(-0.5))));
        assert_eq!(p.line_spacing(), Some(Spacing::Lines(1.2)));
        assert_eq!(p.space_before(), Some(Spacing::Points(6.0)));
        assert_eq!(p.space_after(), Some(Spacing::Points(3.0)));
    }
    {
        let p = tb.paragraph_mut(1).unwrap();
        assert_eq!(p.level(), 1);
        assert_eq!(p.alignment(), Some(Alignment::Right));
        assert_eq!(
            p.bullet(),
            Some(Bullet::Numbered {
                scheme: AutoNumberScheme::RomanUcPeriod,
                start_at: 3
            })
        );
    }
    {
        let mut p = tb.paragraph_mut(2).unwrap();
        assert_eq!(p.run_count(), 6);
        let big = p.run_mut(0).unwrap();
        assert_eq!(big.font_size(), Some(FontSize(40.0)));
        assert!(big.is_bold());
        assert_eq!(big.text_color(), Some(Color::Scheme(SchemeColor::Accent1)));
        let intl = p.run_mut(5).unwrap();
        assert_eq!(intl.fonts(), (Some("Calibri"), Some("MS Mincho"), Some("Arial")));
        assert_eq!(intl.raw().r_pr.as_ref().unwrap().lang.as_deref(), Some("vi-VN"));
    }
    let v = slide.shape_by_id(vertical).unwrap();
    assert_eq!(v.text_direction(), TextDirection::Vertical270);
    assert_eq!(v.autofit(), Some(Autofit::ResizeShape));
}

#[test]
fn connectors_are_glued_to_shapes() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
    let a = slide
        .add_shape(ShapeType::Rect, cm(2.0), cm(2.0), cm(4.0), cm(2.0))
        .id();
    let b = slide
        .add_shape(ShapeType::Ellipse, cm(12.0), cm(8.0), cm(4.0), cm(2.0))
        .id();
    let elbow = {
        let mut c = slide
            .connect_shapes(ConnectorKind::Elbow, a, Side::Right, b, Side::Top)
            .unwrap();
        c.set_line(
            Line::solid(Rgb(0x40, 0x40, 0x40), Length::pt(2.0)).tail(ArrowHead::new(ArrowKind::Triangle)),
        );
        c.id()
    };
    let free = slide
        .add_connector(
            ConnectorKind::Straight,
            (cm(20.0), cm(15.0)),
            (cm(15.0), cm(10.0)),
        )
        .id();
    slide
        .connect_shapes(ConnectorKind::Curved, b, Side::Bottom, a, Side::Left)
        .unwrap();
    assert!(
        slide
            .connect_shapes(ConnectorKind::Straight, a, Side::Top, 99, Side::Top)
            .is_err()
    );

    let mut deck = common::round_trip(&mut deck);
    let slide = deck.slide(0).unwrap();
    let kinds: Vec<ShapeKind> = slide.shapes().iter().map(|s| s.kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == ShapeKind::Connector).count(), 3);
    let info = slide.shapes().into_iter().find(|s| s.id == elbow).unwrap();
    // From the right edge of `a` (6 cm, 3 cm) to the top of `b` (14 cm, 8 cm).
    assert_eq!(info.offset, Some((cm(6.0), cm(3.0))));
    assert_eq!(info.size, Some((cm(8.0), cm(5.0))));
    let mut slide = deck.slide_mut(0).unwrap();
    let c = slide.connector_mut(elbow).unwrap();
    assert_eq!(c.start_connection(), Some((a, 3)));
    assert_eq!(c.end_connection(), Some((b, 0)));
    assert_eq!(c.line_format().unwrap().tail.unwrap().kind, ArrowKind::Triangle);
    let c = slide.connector_mut(free).unwrap();
    let x = c.raw().sp_pr.as_ref().unwrap().xfrm.as_ref().unwrap();
    assert_eq!((x.flip_h, x.flip_v), (Some(true), Some(true)));
    assert!(slide.connector_mut(a).is_none());
}

#[test]
fn groups_nest_and_ungroup_with_coordinate_mapping() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
    let a = slide
        .add_shape(ShapeType::Rect, cm(1.0), cm(1.0), cm(2.0), cm(2.0))
        .id();
    let b = slide
        .add_shape(ShapeType::Ellipse, cm(5.0), cm(1.0), cm(2.0), cm(3.0))
        .id();
    let c = slide
        .add_text_box(cm(1.0), cm(6.0), cm(4.0), cm(1.0), "caption")
        .id();
    let inner = slide.group(&[a, b]).unwrap();
    let outer = slide.group(&[inner, c]).unwrap();
    assert!(slide.group(&[]).is_err());
    assert!(slide.group(&[a]).is_err(), "members of groups are not top-level");

    let mut deck = common::round_trip(&mut deck);
    let slide = deck.slide(0).unwrap();
    let shapes = slide.shapes();
    assert_eq!(shapes.len(), 1);
    assert_eq!(shapes[0].kind, ShapeKind::Group);
    assert_eq!(shapes[0].id, outer);
    assert_eq!(shapes[0].offset, Some((cm(1.0), cm(1.0))));
    assert_eq!(shapes[0].size, Some((cm(6.0), cm(6.0))));
    assert_eq!(shapes[0].children[0].id, inner);
    assert_eq!(shapes[0].children[0].children.len(), 2);
    assert_eq!(shapes[0].text, "caption");
    // chOff/chExt of the group equal its off/ext.
    let pml::CT_GroupShape_Choice::GrpSp(g) = &slide
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
    let x = g.grp_sp_pr.as_ref().unwrap().xfrm.as_ref().unwrap();
    assert_eq!(x.off, x.ch_off);
    assert_eq!(x.ext, x.ch_ext);

    // Shrink the outer group to half size: members are scaled when ungrouping.
    let mut slide = deck.slide_mut(0).unwrap();
    {
        let tree = slide.raw_mut().c_sld.as_mut().unwrap().sp_tree.as_mut().unwrap();
        let pml::CT_GroupShape_Choice::GrpSp(g) = &mut tree.choice[0] else {
            panic!()
        };
        let ext = g
            .grp_sp_pr
            .as_mut()
            .unwrap()
            .xfrm
            .as_mut()
            .unwrap()
            .ext
            .as_mut()
            .unwrap();
        ext.cx = Some(cm(3.0).as_emu());
        ext.cy = Some(cm(3.0).as_emu());
    }
    let members = slide.ungroup(outer).unwrap();
    assert_eq!(members, vec![inner, c]);
    let members = slide.ungroup(inner).unwrap();
    assert_eq!(members, vec![a, b]);
    assert!(slide.ungroup(inner).is_err());
    let shapes = slide.shapes();
    assert_eq!(shapes.len(), 3);
    let find = |id: u32| shapes.iter().find(|s| s.id == id).unwrap();
    assert_eq!(find(a).offset, Some((cm(1.0), cm(1.0))));
    assert_eq!(find(a).size, Some((cm(1.0), cm(1.0))));
    assert_eq!(find(b).offset, Some((cm(3.0), cm(1.0))));
    assert_eq!(find(c).offset, Some((cm(1.0), cm(3.5))));
    common::round_trip(&mut deck);
}

#[test]
fn pictures_crop_transparency_effects_and_replacement() {
    let mut deck = Presentation::new();
    let red = tiny_png(4, 2);
    let blue = tiny_png(2, 2);
    let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
    let id = slide.add_picture(&red, cm(1.0), cm(1.0), cm(8.0), None).unwrap();
    {
        let mut pic = slide.picture_mut(id).unwrap();
        pic.set_crop(Crop {
            left: 0.1,
            top: 0.0,
            right: 0.1,
            bottom: 0.05,
        })
        .set_transparency(0.3)
        .set_line(Line::solid(Rgb::BLACK, Length::pt(2.0)))
        .set_shadow(Some(Shadow::default()))
        .set_soft_edges(Some(Length::pt(4.0)))
        .set_alt_text(Some("Red"), "A red rectangle");
    }
    let shape = slide
        .add_shape(ShapeType::Ellipse, cm(10.0), cm(1.0), cm(4.0), cm(4.0))
        .id();
    slide.set_shape_picture_fill(shape, &blue).unwrap();
    assert!(slide.set_alt_text(shape, None, "Blue circle"));
    assert!(!slide.set_alt_text(999, None, "missing"));
    let before_images = image_parts(&deck);

    let mut deck = common::round_trip(&mut deck);
    let mut slide = deck.slide_mut(0).unwrap();
    {
        let pic = slide.picture_mut(id).unwrap();
        assert!((pic.crop().left - 0.1).abs() < 1e-9);
        assert!((pic.crop().bottom - 0.05).abs() < 1e-9);
        assert!((pic.transparency() - 0.3).abs() < 1e-9);
        assert_eq!(pic.soft_edges(), Some(Length::pt(4.0)));
        assert_eq!(pic.alt_text(), Some("A red rectangle"));
        assert!(pic.shadow().is_some());
    }
    assert_eq!(slide.shape_by_id(shape).unwrap().alt_text(), Some("Blue circle"));
    // Replacing the picture with the image the ellipse uses drops the red image part.
    slide.replace_picture(id, &blue).unwrap();
    assert!(slide.replace_picture(shape, &blue).is_err(), "not a picture");
    assert_eq!(before_images, 2);
    assert_eq!(image_parts(&deck), 1, "the unused image part is removed");
    let deck = common::round_trip(&mut deck);
    let pic_rels = deck
        .package()
        .relationships(Some(deck.slide(0).unwrap().part_name()))
        .unwrap()
        .by_type(openxml_opc::known::rel_types::IMAGE)
        .count();
    assert_eq!(pic_rels, 1);
}

fn image_parts(deck: &Presentation) -> usize {
    deck.package()
        .parts()
        .filter(|(n, _)| n.as_str().starts_with("/ppt/media/image"))
        .count()
}

#[test]
fn picture_placeholders_are_filled_and_cropped() {
    let mut deck = Presentation::new();
    // The built-in template has no picture layout: add one.
    deck.add_layout("Picture", LayoutKind::TitleOnly)
        .unwrap()
        .add_placeholder(
            openxml_pptx::PlaceholderKind::Picture,
            cm(2.0),
            cm(4.0),
            cm(10.0),
            cm(10.0),
        );
    let mut slide = deck.add_slide("Picture").unwrap();
    let wide = tiny_png(40, 10);
    let id = slide.fill_picture_placeholder(&wide).unwrap();
    // A second picture needs a second placeholder.
    assert!(slide.fill_picture_placeholder(&wide).is_err());
    let mut deck = common::round_trip(&mut deck);
    let slide = deck.slide(0).unwrap();
    let info = slide.shapes().into_iter().find(|s| s.id == id).unwrap();
    assert_eq!(info.kind, ShapeKind::Picture);
    assert_eq!(
        info.placeholder.unwrap().kind,
        openxml_pptx::PlaceholderKind::Picture
    );
    let mut slide = deck.slide_mut(0).unwrap();
    let crop = slide.picture_mut(id).unwrap().crop();
    assert!(
        crop.left > 0.0 && (crop.left - crop.right).abs() < 1e-9,
        "{crop:?}"
    );
    assert_eq!(crop.top, 0.0);
    let mut blank = Presentation::new();
    let mut slide = blank.add_slide(LayoutKind::Blank).unwrap();
    assert!(slide.fill_picture_placeholder(&wide).is_err());
}

#[test]
fn grouping_rules_follow_powerpoint() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::TitleOnly).unwrap();
    slide.set_title("Title").unwrap();
    let title = slide.shapes()[0].id;
    let a = slide
        .add_shape(ShapeType::Rect, cm(1.0), cm(5.0), cm(2.0), cm(2.0))
        .id();
    let b = slide
        .add_shape(ShapeType::Rect, cm(4.0), cm(5.0), cm(2.0), cm(2.0))
        .id();
    let table = slide
        .add_table(1, 1, cm(8.0), cm(5.0), cm(2.0), cm(1.0))
        .unwrap()
        .id();
    assert!(
        slide.group(&[a, title]).is_err(),
        "placeholders cannot be grouped"
    );
    assert!(
        slide.group(&[a, table]).is_err(),
        "tables are locked against grouping"
    );
    slide
        .add_animation(a, openxml_pptx::Animation::new(openxml_pptx::Effect::FadeIn))
        .unwrap();
    let group = slide.group(&[a, b]).unwrap();
    assert!(
        slide.animations().is_empty(),
        "grouping removes the members' animations"
    );
    assert!(
        slide
            .add_animation(a, openxml_pptx::Animation::new(openxml_pptx::Effect::FadeIn))
            .is_err(),
        "members of groups are animated through their group"
    );
    slide
        .add_animation(group, openxml_pptx::Animation::new(openxml_pptx::Effect::FadeIn))
        .unwrap();
    slide.ungroup(group).unwrap();
    assert!(
        slide.animations().is_empty(),
        "ungrouping removes the group's animations"
    );
    common::round_trip(&mut deck);
}
