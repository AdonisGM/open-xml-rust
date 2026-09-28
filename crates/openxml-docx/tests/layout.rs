//! Drawings, shapes, sections, equations, styles, lists, document settings
//! and hyperlinks: created, saved, validated against the schemas, reopened
//! and checked; real documents read and edited.

mod common;

use common::{assert_untouched_except, open, part_xml, pn, save_validate_reopen};
use openxml_core::image::tiny_png;
use openxml_docx::*;
use openxml_opc::Package;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::{dml, dml_picture};
use openxml_xml::{Ns, RawElement};

/// A 1×1 GIF.
const GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x00,
    0x00, 0x00, 0x21, 0xF9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3B,
];

// ----- pictures ---------------------------------------------------------------------------

#[test]
fn floating_pictures_with_every_wrapping() {
    let mut doc = Document::new();
    let png = tiny_png(40, 20);
    let placements = [
        (
            Wrap::Square,
            HorizontalPosition::Align(HorizontalAnchor::Margin, HorizontalAlignment::Right),
            VerticalPosition::Offset(VerticalAnchor::Paragraph, Length::ZERO),
        ),
        (
            Wrap::Tight,
            HorizontalPosition::Offset(HorizontalAnchor::Column, Length::cm(1.0)),
            VerticalPosition::Offset(VerticalAnchor::Paragraph, Length::cm(0.5)),
        ),
        (
            Wrap::TopAndBottom,
            HorizontalPosition::Align(HorizontalAnchor::Page, HorizontalAlignment::Center),
            VerticalPosition::Align(VerticalAnchor::Margin, VerticalAlignment::Top),
        ),
        (
            Wrap::BehindText,
            HorizontalPosition::Offset(HorizontalAnchor::Page, Length::inches(1.0)),
            VerticalPosition::Offset(VerticalAnchor::Page, Length::inches(2.0)),
        ),
        (
            Wrap::InFrontOfText,
            HorizontalPosition::Align(HorizontalAnchor::Margin, HorizontalAlignment::Left),
            VerticalPosition::Align(VerticalAnchor::Line, VerticalAlignment::Center),
        ),
    ];
    for (i, (wrap, h, v)) in placements.iter().enumerate() {
        let mut options = PictureOptions::new(Length::cm(3.0));
        options.height = (i == 0).then(|| Length::cm(2.0));
        options.description = Some(format!("Picture {i}"));
        options.title = Some(format!("Title {i}"));
        options.floating = Some(Floating::new(*h, *v, *wrap));
        doc.add_paragraph(&format!("Paragraph {i} with text around the picture."))
            .add_picture_with(&png, &options)
            .unwrap();
    }
    let mut inline = PictureOptions::new(Length::cm(2.0));
    inline.description = Some("Inline alt text".into());
    inline.hyperlink = Some("https://example.com/logo".into());
    doc.add_picture(&png, Length::cm(1.0)).unwrap();
    doc.add_picture_with(&png, &inline).unwrap();
    let bad = PictureOptions::new(Length::ZERO);
    assert!(doc.add_paragraph("").add_picture_with(&png, &bad).is_err());

    let back = save_validate_reopen(&mut doc);
    let pictures = back.pictures();
    assert_eq!(pictures.len(), 7);
    for (i, p) in pictures.iter().take(5).enumerate() {
        assert!(p.floating);
        assert_eq!(p.description.as_deref(), Some(format!("Picture {i}").as_str()));
        assert_eq!(p.title.as_deref(), Some(format!("Title {i}").as_str()));
        assert_eq!(p.width, Length::cm(3.0));
    }
    assert_eq!(pictures[0].height, Length::cm(2.0));
    assert_eq!(pictures[1].height, Length::cm(1.5));
    assert!(!pictures[5].floating && !pictures[6].floating);
    assert_eq!(pictures[6].description.as_deref(), Some("Inline alt text"));
    let ids: std::collections::BTreeSet<u32> = pictures.iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), 7, "drawing ids are unique");
    let xml = part_xml(back.package(), "/word/document.xml");
    for tag in [
        "<wp:wrapSquare ",
        "<wp:wrapTight ",
        "<wp:wrapTopAndBottom/>",
        "<wp:wrapNone/>",
    ] {
        assert!(xml.contains(tag), "{tag}");
    }
    assert!(
        xml.contains(r#"behindDoc="1""#) || xml.contains(r#"behindDoc="true""#),
        "behind text"
    );
    assert!(xml.contains("<wp:align>right</wp:align>"));
    assert!(xml.contains("<wp:posOffset>360000</wp:posOffset>"));
    assert!(xml.contains(r#"relativeFrom="line""#));
    // The image hyperlink is an external relationship of the main part.
    assert!(xml.contains("<a:hlinkClick "));
    let main = back.package().main_part().unwrap();
    let rels = back.package().relationships(Some(&main)).unwrap();
    assert!(
        rels.iter()
            .any(|r| r.is_external() && r.target == "https://example.com/logo")
    );
}

#[test]
fn alt_text_and_picture_replacement() {
    let mut doc = Document::new();
    doc.add_picture(&tiny_png(10, 10), Length::cm(2.0)).unwrap();
    doc.add_picture(&tiny_png(10, 10), Length::cm(2.0)).unwrap();
    doc.set_picture_alt_text(1, Some("Chart of sales"), Some("Sales"))
        .unwrap();
    assert!(doc.set_picture_alt_text(5, None, None).is_err());
    let old_part = doc.pictures()[0].image_part.clone().unwrap();
    doc.replace_picture(0, GIF).unwrap();
    assert!(doc.replace_picture(0, b"not an image").is_err());
    assert!(doc.replace_picture(9, GIF).is_err());

    let back = save_validate_reopen(&mut doc);
    let pictures = back.pictures();
    assert_eq!(pictures[1].description.as_deref(), Some("Chart of sales"));
    assert_eq!(pictures[1].title.as_deref(), Some("Sales"));
    let new_part = pictures[0].image_part.clone().unwrap();
    assert!(new_part.as_str().ends_with(".gif"), "{new_part}");
    assert_eq!(
        back.package().part(&new_part).unwrap().content_type(),
        "image/gif"
    );
    assert_eq!(back.package().part(&new_part).unwrap().data(), GIF);
    // The frame keeps its size and the unused PNG part is gone.
    assert_eq!(pictures[0].width, Length::cm(2.0));
    assert!(!back.package().contains(&old_part));
    assert!(back.package().contains(pictures[1].image_part.as_ref().unwrap()));
}

#[test]
fn drawing_fixture_pictures_are_read_and_replaced() {
    let doc = open("drawing.docx");
    let pictures = doc.pictures();
    assert!(pictures.len() >= 10, "{}", pictures.len());
    let descriptions: Vec<&str> = pictures.iter().filter_map(|p| p.description.as_deref()).collect();
    assert!(descriptions.contains(&"cbonds_logo_eng"));
    assert!(descriptions.contains(&"RFCM"));
    assert!(pictures.iter().any(|p| p.floating));
    let logo = pictures
        .iter()
        .position(|p| p.description.as_deref() == Some("cbonds_logo_eng"))
        .unwrap();
    let old = pictures[logo].image_part.clone().unwrap();

    let original = Package::open_path(common::fixture("drawing.docx")).unwrap();
    let mut doc = doc;
    doc.replace_picture(logo, &tiny_png(8, 8)).unwrap();
    let bytes = doc.to_bytes().unwrap();
    let after = Package::from_bytes(&bytes).unwrap();
    assert!(!after.contains(&old), "the replaced image was only used once");
    assert_untouched_except(&original, &after, &["/word/document.xml", old.as_str()]);
    let back = Document::from_bytes(&bytes).unwrap();
    assert_eq!(
        back.pictures()[logo].description.as_deref(),
        Some("cbonds_logo_eng")
    );
    assert!(
        back.pictures()[logo]
            .image_part
            .as_ref()
            .unwrap()
            .as_str()
            .ends_with(".png")
    );
}

#[test]
fn any_graphic_can_be_framed_for_later_chart_support() {
    // The generic frame takes any a:graphicData; here a picture built by hand.
    let mut doc = Document::new();
    let main = doc.main_part_name().clone();
    let image = pn("/word/media/custom.png");
    doc.package_mut()
        .add_part(image.clone(), "image/png", tiny_png(4, 4))
        .unwrap();
    let rel = doc
        .package_mut()
        .add_relationship(Some(&main), rel_types::IMAGE, &image)
        .unwrap();
    let pic = dml_picture::CT_Picture {
        nv_pic_pr: Some(Box::new(dml_picture::CT_PictureNonVisual {
            c_nv_pr: Some(Box::new(dml::CT_NonVisualDrawingProps {
                id: Some(0),
                name: Some("custom.png".into()),
                ..Default::default()
            })),
            c_nv_pic_pr: Some(Box::default()),
            ..Default::default()
        })),
        blip_fill: Some(Box::new(dml::CT_BlipFillProperties {
            blip: Some(Box::new(dml::CT_Blip {
                r_embed: Some(rel),
                ..Default::default()
            })),
            ..Default::default()
        })),
        sp_pr: Some(Box::default()),
        ..Default::default()
    };
    let data = dml::CT_GraphicalObjectData {
        uri: Some("http://schemas.openxmlformats.org/drawingml/2006/picture".into()),
        any: vec![RawElement::from_typed(&pic, Ns::PIC, "pic")],
        ..Default::default()
    };
    let floating = Floating::new(
        HorizontalPosition::Align(HorizontalAnchor::Margin, HorizontalAlignment::Center),
        VerticalPosition::Offset(VerticalAnchor::Paragraph, Length::ZERO),
        Wrap::TopAndBottom,
    );
    let mut p = doc.add_paragraph("");
    let a = p.add_graphic(data.clone(), Length::cm(1.0), Length::cm(1.0), "Graphic A", None);
    let b = p.add_graphic(
        data,
        Length::cm(1.0),
        Length::cm(1.0),
        "Graphic B",
        Some(&floating),
    );
    assert!(b > a);
    let back = save_validate_reopen(&mut doc);
    let pictures = back.pictures();
    assert_eq!(pictures.len(), 2);
    assert_eq!(pictures[1].name.as_deref(), Some("Graphic B"));
    assert!(pictures[1].floating);
    assert_eq!(pictures[0].image_part.as_ref(), Some(&image));
}

// ----- text boxes, shapes and watermarks --------------------------------------------------

#[test]
fn text_boxes_and_simple_shapes_are_valid_vml() {
    let mut doc = Document::new();
    let mut options = ShapeOptions::new(Length::inches(2.5), Length::inches(1.0));
    options.fill = Some("FFF2CC".into());
    options.position = Some((Length::inches(4.0), Length::ZERO));
    options.hyperlink = Some("https://example.com".into());
    doc.add_paragraph("Body text")
        .add_text_box(&options, |content| {
            content.add_paragraph("Box title").add_run("!").bold(true);
            content
                .add_paragraph("Second line")
                .set_alignment(Alignment::Center);
            Ok(())
        })
        .unwrap();
    doc.add_paragraph("")
        .add_text_box(&ShapeOptions::new(Length::cm(3.0), Length::cm(1.0)), |_| Ok(()))
        .unwrap();
    let mut p = doc.add_paragraph("");
    for (kind, wrap) in [
        (ShapeKind::Rectangle, Wrap::Square),
        (ShapeKind::RoundedRectangle, Wrap::Tight),
        (ShapeKind::Ellipse, Wrap::TopAndBottom),
        (ShapeKind::Line, Wrap::InFrontOfText),
    ] {
        let mut o = ShapeOptions::new(Length::cm(2.0), Length::cm(1.0));
        o.fill = Some("5B9BD5".into());
        o.stroke = (kind != ShapeKind::Ellipse).then(|| "1F4E79".into());
        o.stroke_weight = Length::pt(1.5);
        o.position = Some((Length::cm(1.0), Length::cm(1.0)));
        o.wrap = wrap;
        p.add_shape(kind, &o).unwrap();
    }
    p.add_shape(
        ShapeKind::Rectangle,
        &ShapeOptions::new(Length::cm(1.0), Length::cm(1.0)),
    )
    .unwrap();

    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.text_boxes(), ["Box title!\nSecond line", ""]);
    // Text boxes are not body text.
    assert_eq!(back.text(), "Body text\n\n");
    let xml = part_xml(back.package(), "/word/document.xml");
    for tag in [
        "<v:rect ",
        "<v:roundrect ",
        "<v:oval ",
        "<v:line ",
        "<v:textbox>",
        "<w:txbxContent>",
    ] {
        assert!(xml.contains(tag), "{tag}");
    }
    assert!(xml.contains(r#"href="https://example.com""#));
    assert!(xml.contains(r#"<w10:wrap type="tight"/>"#));
}

#[test]
fn text_watermark_in_headers() {
    let mut doc = Document::new();
    doc.add_paragraph("Draft body");
    doc.set_watermark("CONFIDENTIAL").unwrap();
    assert!(doc.set_watermark("").is_err());
    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.watermark().as_deref(), Some("CONFIDENTIAL"));
    let header = back.header().unwrap();
    let xml = part_xml(back.package(), header.part_name().as_str());
    assert!(xml.contains(r#"o:spt="136""#) && xml.contains(r#"string="CONFIDENTIAL""#));
    // Replacing keeps one watermark per header.
    let mut edit = back;
    edit.set_watermark("DRAFT").unwrap();
    let back = save_validate_reopen(&mut edit);
    let xml = part_xml(back.package(), back.header().unwrap().part_name().as_str());
    assert_eq!(xml.matches(r#"id="PowerPlusWaterMarkObject"#).count(), 1);
    assert_eq!(xml.matches("<v:shapetype ").count(), 1);
    assert_eq!(back.watermark().as_deref(), Some("DRAFT"));
    let mut edit = back;
    assert!(edit.remove_watermark());
    assert!(!edit.remove_watermark());
    let back = save_validate_reopen(&mut edit);
    assert_eq!(back.watermark(), None);
}

// ----- sections ---------------------------------------------------------------------------

#[test]
fn sections_breaks_columns_numbering_and_header_variants() {
    let mut doc = Document::new();
    doc.add_paragraph("Cover");
    doc.set_header("Default header").unwrap();
    {
        let mut s = doc.last_section_mut();
        s.set_title_page(true);
        s.set_header(HeaderFooterType::First, "Cover header").unwrap();
        s.set_page_borders(Some(&PageBorders::around(
            Border::new(BorderStyle::Double, 1.5, "1F4E79").with_space(24),
        )))
        .unwrap();
    }
    for (i, kind) in [
        SectionBreak::NextPage,
        SectionBreak::Continuous,
        SectionBreak::EvenPage,
        SectionBreak::OddPage,
    ]
    .into_iter()
    .enumerate()
    {
        let mut s = doc.add_section(kind);
        if i == 0 {
            s.set_page_setup(&PageSetup::a4().landscape());
            s.set_columns(&Columns {
                count: 2,
                spacing: Length::cm(1.0),
                separator: true,
            });
            s.set_page_numbering(&PageNumbering {
                format: Some(NumberFormat::LowerRoman),
                start: Some(1),
            });
            s.set_line_numbering(Some(&LineNumbering {
                count_by: 5,
                start: 1,
                distance: Some(Length::cm(0.5)),
                restart: LineNumberRestart::EachSection,
            }));
            s.set_header(HeaderFooterType::Even, "Even header").unwrap();
            s.set_footer(HeaderFooterType::Default, "Section footer").unwrap();
        }
        if i == 3 {
            // The copied default header is shared: this section gets its own.
            s.set_header(HeaderFooterType::Default, "Last header").unwrap();
        }
        doc.add_paragraph(&format!("Section {}", i + 1));
    }

    let back = save_validate_reopen(&mut doc);
    let sections = back.sections();
    assert_eq!(sections.len(), 5);
    let kinds: Vec<SectionBreak> = sections.iter().map(|s| s.break_type()).collect();
    assert_eq!(
        kinds,
        [
            SectionBreak::NextPage,
            SectionBreak::NextPage,
            SectionBreak::Continuous,
            SectionBreak::EvenPage,
            SectionBreak::OddPage
        ]
    );
    assert!(sections[0].title_page());
    assert!(sections[0].page_borders().unwrap().top.is_some());
    assert_eq!(sections[0].page_borders().unwrap().top.unwrap().space, 24);
    let s1 = &sections[1];
    assert_eq!(s1.page_setup().orientation, Orientation::Landscape);
    assert_eq!(
        s1.columns(),
        Columns {
            count: 2,
            spacing: Length::twips(Length::cm(1.0).as_twips()),
            separator: true
        }
    );
    assert_eq!(
        s1.page_numbering(),
        Some(PageNumbering {
            format: Some(NumberFormat::LowerRoman),
            start: Some(1)
        })
    );
    let ln = s1.line_numbering().unwrap();
    assert_eq!(
        (ln.count_by, ln.start, ln.restart),
        (5, 1, LineNumberRestart::EachSection)
    );
    assert!(
        sections[2].page_numbering().is_none(),
        "numbering restarts belong to one section"
    );
    assert!(back.even_and_odd_headers());
    let text = |s, k, t| back.header_footer(s, k, t).map(|h| h.text());
    assert_eq!(
        text(0, HeaderFooterKind::Header, HeaderFooterType::Default).as_deref(),
        Some("Default header")
    );
    assert_eq!(
        text(0, HeaderFooterKind::Header, HeaderFooterType::First).as_deref(),
        Some("Cover header")
    );
    assert_eq!(
        text(1, HeaderFooterKind::Header, HeaderFooterType::Even).as_deref(),
        Some("Even header")
    );
    assert_eq!(
        text(1, HeaderFooterKind::Footer, HeaderFooterType::Default).as_deref(),
        Some("Section footer")
    );
    // Later sections copy the references of the previous one.
    assert_eq!(
        text(3, HeaderFooterKind::Header, HeaderFooterType::Default).as_deref(),
        Some("Default header")
    );
    assert_eq!(
        text(4, HeaderFooterKind::Header, HeaderFooterType::Default).as_deref(),
        Some("Last header")
    );
    assert_eq!(back.headers_and_footers().len(), 5);
    assert_eq!(
        back.text(),
        "Cover\n\nSection 1\n\nSection 2\n\nSection 3\n\nSection 4"
    );
    let mut back = back;
    assert!(back.section_mut(9).is_none());
    assert_eq!(back.section_mut(1).unwrap().index(), 1);
}

#[test]
fn headers_fixture_has_one_header_per_section() {
    let doc = open("Headers.docx");
    let sections = doc.sections();
    assert_eq!(sections.len(), 3);
    for (i, _) in sections.iter().enumerate() {
        let header = doc
            .header_footer(i, HeaderFooterKind::Header, HeaderFooterType::Default)
            .unwrap();
        assert_eq!(header.text().trim_end(), format!("Section {}", i + 1));
    }
}

// ----- equations --------------------------------------------------------------------------

#[test]
fn equations_inline_and_display() {
    let mut doc = Document::new();
    let pythagoras = Math::row([
        Math::superscript(Math::text("a"), Math::text("2")),
        Math::text("+"),
        Math::superscript(Math::text("b"), Math::text("2")),
        Math::text("="),
        Math::superscript(Math::text("c"), Math::text("2")),
    ]);
    doc.add_paragraph("Inline: ")
        .add_equation(&pythagoras)
        .add_text(" holds.");
    let everything = Math::row([
        Math::frac(Math::text("1"), Math::text("n")),
        Math::sum(
            Some(Math::text("i=1")),
            Some(Math::text("n")),
            Math::subscript(Math::text("x"), Math::text("i")),
        ),
        Math::integral(Some(Math::text("0")), Some(Math::text("∞")), Math::text("f(x)dx")),
        Math::sub_superscript(Math::text("y"), Math::text("j"), Math::text("2")),
        Math::sqrt(Math::text("2")),
        Math::root(Math::text("3"), Math::text("8")),
        Math::parens(Math::text("a+b")),
        Math::delimited('[', ']', [Math::text("u"), Math::text("v")]),
        Math::matrix([
            vec![Math::text("1"), Math::text("0")],
            vec![Math::text("0"), Math::text("1")],
        ]),
    ]);
    doc.add_equation(&everything);

    let back = save_validate_reopen(&mut doc);
    let equations = back.equations();
    assert_eq!(equations.len(), 2);
    assert_eq!(equations[0], "a^(2)+b^(2)=c^(2)");
    assert_eq!(
        equations[1],
        "(1)/(n)∑_(i=1)^n x_(i)∫_0^∞ f(x)dxy_(j)^(2)√(2)√(3&8)(a+b)[u|v]■(1&0@0&1)"
    );
    assert_eq!(back.paragraphs()[0].equations().len(), 1);
    assert_eq!(back.paragraphs()[0].text(), "Inline:  holds.");
    let xml = part_xml(back.package(), "/word/document.xml");
    for tag in [
        "<m:oMathPara>",
        "<m:f>",
        "<m:nary>",
        "<m:rad>",
        "<m:d>",
        "<m:m>",
        "<m:sSubSup>",
    ] {
        assert!(xml.contains(tag), "{tag}");
    }
}

// ----- styles and formatting --------------------------------------------------------------

#[test]
fn custom_styles_and_paragraph_formatting() {
    let mut doc = Document::new();
    let mut callout = StyleDefinition::paragraph("Callout", "Callout");
    callout.next = Some("Normal".into());
    callout.paragraph = ParagraphFormat {
        borders: Some(ParagraphBorders::around(
            Border::single(1.0, "2F5496").with_space(4),
        )),
        shading: Some("DEEAF6".into()),
        space_before: Some(Length::pt(6.0)),
        space_after: Some(Length::pt(6.0)),
        keep_together: Some(true),
        ..Default::default()
    };
    callout.run = RunFormat {
        italic: Some(true),
        color: Some("1F3864".into()),
        ..Default::default()
    };
    doc.add_style(&callout).unwrap();
    let mut code = StyleDefinition::character("CodeChar", "Code Char");
    code.run = RunFormat {
        font: Some("Consolas".into()),
        size: Some(FontSize(10.0)),
        ..Default::default()
    };
    doc.add_style(&code).unwrap();
    let mut grid = StyleDefinition::table("LightGrid", "Light Grid");
    grid.table = TableFormat {
        borders: Some(TableBorders::all(Border::single(0.5, "A5A5A5"))),
        cell_margins: Some(CellMargins::all(Length::pt(3.0))),
        ..Default::default()
    };
    grid.paragraph.space_after = Some(Length::ZERO);
    doc.add_style(&grid).unwrap();

    doc.add_paragraph("Styled callout").set_style("Callout").unwrap();
    doc.add_paragraph("").add_run("let x = 1;").style("CodeChar");
    let spacings = [
        LineSpacing::Single,
        LineSpacing::OnePointFive,
        LineSpacing::Double,
        LineSpacing::Multiple(1.15),
        LineSpacing::Exactly(Length::pt(14.0)),
        LineSpacing::AtLeast(Length::pt(18.0)),
    ];
    for s in spacings {
        doc.add_paragraph("spacing").set_line_spacing(s);
    }
    let mut p = doc.add_paragraph("Name\tPrice");
    p.add_tab_stop(TabStop::new(Length::inches(3.0), TabAlignment::Right).with_leader(TabLeader::Dot))
        .add_tab_stop(TabStop::new(Length::inches(1.0), TabAlignment::Left))
        .set_keep_together(true)
        .set_keep_with_next(true);
    p.set_borders(&ParagraphBorders {
        bottom: Some(Border::new(BorderStyle::Double, 0.75, "000000")),
        ..Default::default()
    })
    .unwrap()
    .set_shading("F2F2F2")
    .unwrap();
    doc.add_paragraph("formatted")
        .set_format(&ParagraphFormat {
            alignment: Some(Alignment::Right),
            indent_left: Some(Length::cm(1.0)),
            indent_right: Some(Length::cm(1.0)),
            first_line_indent: Some(Length::cm(-0.5)),
            page_break_before: Some(false),
            widow_control: Some(true),
            outline_level: Some(1),
            ..Default::default()
        })
        .unwrap();

    let back = save_validate_reopen(&mut doc);
    let style = back.style("Callout").unwrap();
    assert_eq!(style.type_, Some(openxml_schema::wml::ST_StyleType::Paragraph));
    assert!(style.p_pr.as_ref().unwrap().p_bdr.is_some());
    assert!(back.style("CodeChar").unwrap().r_pr.is_some());
    assert!(
        back.style("LightGrid")
            .unwrap()
            .tbl_pr
            .as_ref()
            .unwrap()
            .tbl_cell_mar
            .is_some()
    );
    let paragraphs = back.paragraphs();
    assert_eq!(paragraphs[0].style_id(), Some("Callout"));
    assert_eq!(paragraphs[1].runs()[0].style_id(), Some("CodeChar"));
    for (i, s) in spacings.iter().enumerate() {
        assert_eq!(paragraphs[2 + i].line_spacing(), Some(*s));
    }
    let tabbed = &paragraphs[8];
    assert_eq!(tabbed.tab_stops().len(), 2);
    assert_eq!(tabbed.tab_stops()[0].position, Length::inches(1.0));
    assert_eq!(tabbed.tab_stops()[1].leader, TabLeader::Dot);
    assert!(tabbed.keep_together() && tabbed.keep_with_next());
    assert_eq!(
        tabbed.borders().unwrap().bottom.unwrap().style,
        BorderStyle::Double
    );
    assert_eq!(tabbed.shading().as_deref(), Some("F2F2F2"));
    assert_eq!(paragraphs[9].alignment(), Some(Alignment::Right));
    assert_eq!(paragraphs[9].outline_level(), Some(1));
}

#[test]
fn table_formatting_and_nested_tables() {
    let mut doc = Document::new();
    let mut table = doc.add_table(4, 3).unwrap();
    table
        .set_borders(&TableBorders::all(Border::single(0.5, "808080")))
        .unwrap()
        .set_cell_margins(&CellMargins {
            left: Some(Length::pt(6.0)),
            right: Some(Length::pt(6.0)),
            ..Default::default()
        })
        .set_alignment(TableAlignment::Center)
        .set_layout(TableLayout::Fixed)
        .set_width(TableWidth::Percent(80.0));
    table
        .set_row_height(0, Length::cm(1.0), HeightRule::Exact)
        .unwrap();
    table
        .set_row_height(1, Length::cm(0.8), HeightRule::AtLeast)
        .unwrap();
    table.set_header_rows(2).unwrap();
    table.set_cant_split(3, true).unwrap();
    assert!(
        table
            .set_row_height(9, Length::cm(1.0), HeightRule::Exact)
            .is_err()
    );
    {
        let mut cell = table.cell(2, 1).unwrap();
        cell.set_borders(&TableBorders::outside(Border::single(2.0, "C00000")))
            .unwrap();
        cell.set_margins(&CellMargins::all(Length::pt(2.0)));
        cell.set_text("outer");
        let mut nested = cell.add_table(2, 2).unwrap();
        nested.cell(0, 0).unwrap().set_text("inner");
        nested.set_layout(TableLayout::Autofit);
    }
    doc.add_table(1, 1)
        .unwrap()
        .set_format(&TableFormat {
            indent: Some(Length::cm(1.0)),
            shading: Some("EEEEEE".into()),
            width: Some(TableWidth::Fixed(Length::cm(5.0))),
            ..Default::default()
        })
        .unwrap()
        .set_indent(Length::cm(2.0));

    let back = save_validate_reopen(&mut doc);
    let tables = back.tables();
    let t = tables[0];
    assert_eq!(t.alignment(), Some(TableAlignment::Center));
    assert_eq!(t.layout(), Some(TableLayout::Fixed));
    assert_eq!(
        t.borders().unwrap().inside_vertical.unwrap().color.as_deref(),
        Some("808080")
    );
    let rows = t.rows();
    assert_eq!(
        rows[0].height(),
        Some((Length::twips(Length::cm(1.0).as_twips()), HeightRule::Exact))
    );
    assert_eq!(rows[1].height().unwrap().1, HeightRule::AtLeast);
    assert!(rows[0].is_header() && rows[1].is_header() && !rows[2].is_header());
    assert!(rows[3].cant_split());
    let cell = t.cell(2, 1).unwrap();
    assert_eq!(cell.tables().len(), 1);
    assert_eq!(cell.tables()[0].cell(0, 0).unwrap().text(), "inner");
    assert_eq!(cell.tables()[0].layout(), Some(TableLayout::Autofit));
    // A cell ends with a paragraph after a nested table.
    assert!(matches!(
        cell.raw().block_level_elts.last(),
        Some(openxml_schema::wml::EG_BlockLevelElts::P(_))
    ));
    assert_eq!(cell.text(), "outer\ninner\t\n\t\n");
    assert!(tables[1].raw().tbl_pr.as_ref().unwrap().tbl_ind.is_some());
}

// ----- lists ------------------------------------------------------------------------------

#[test]
fn custom_lists_and_heading_outline() {
    let mut doc = Document::new();
    let mut def = ListDefinition::numbered();
    def.levels[0] = ListLevel::numbered(NumberFormat::UpperRoman, "%1.", 0);
    def.levels[0].start = 4;
    def.levels[1] = ListLevel::numbered(NumberFormat::DecimalZero, "%1.%2)", 1);
    let numbered = doc.add_list_definition(&def).unwrap();
    let mut bullets = ListDefinition::bullets();
    bullets.levels[0] = ListLevel::bullet('\u{F0A7}', 0);
    bullets.levels[0].font = Some("Wingdings".into());
    bullets.levels[0].indent = Length::cm(1.5);
    bullets.levels[0].hanging = Length::cm(0.5);
    let bulleted = doc.add_list_definition(&bullets).unwrap();
    doc.add_list_paragraph("Fourth", numbered, 0).unwrap();
    doc.add_list_paragraph("Nested", numbered, 1).unwrap();
    doc.add_list_paragraph("Square bullet", bulleted, 0).unwrap();
    let restarted = doc.restart_numbering(numbered).unwrap();
    doc.add_list_paragraph("Again fourth", restarted, 0).unwrap();
    doc.add_paragraph("manual").set_list(bulleted, 2).unwrap();
    let outline = doc.number_headings(3, None).unwrap();
    doc.add_heading("Chapter", 1).unwrap();
    doc.add_heading("Section", 2).unwrap();

    let back = save_validate_reopen(&mut doc);
    let paragraphs = back.paragraphs();
    assert_eq!(paragraphs[0].numbering(), Some((numbered, 0)));
    assert_eq!(paragraphs[1].numbering(), Some((numbered, 1)));
    assert_eq!(paragraphs[2].numbering(), Some((bulleted, 0)));
    assert_eq!(paragraphs[3].numbering(), Some((restarted, 0)));
    assert_eq!(paragraphs[4].numbering(), Some((bulleted, 2)));
    let numbering = back.numbering().unwrap();
    assert_eq!(numbering.abstract_num.len(), 3);
    let first = &numbering.abstract_num[0].lvl[0];
    assert_eq!(
        first.num_fmt.as_ref().unwrap().val,
        Some(NumberFormat::UpperRoman)
    );
    assert_eq!(first.start.as_ref().unwrap().val, Some(4));
    let bullet = &numbering.abstract_num[1].lvl[0];
    assert_eq!(bullet.lvl_text.as_ref().unwrap().val.as_deref(), Some("\u{F0A7}"));
    assert!(bullet.r_pr.is_some());
    let restart = numbering
        .num
        .iter()
        .find(|n| n.num_id == Some(restarted))
        .unwrap();
    assert_eq!(
        restart.lvl_override[0].start_override.as_ref().unwrap().val,
        Some(1)
    );
    let outline_def = &numbering.abstract_num[2];
    assert_eq!(outline_def.lvl.len(), 3);
    assert_eq!(
        outline_def.lvl[1].p_style.as_ref().unwrap().val.as_deref(),
        Some("Heading2")
    );
    assert_eq!(
        outline_def.lvl[2].lvl_text.as_ref().unwrap().val.as_deref(),
        Some("%1.%2.%3.")
    );
    let h1 = back.style("Heading1").unwrap();
    let num_pr = h1.p_pr.as_ref().unwrap().num_pr.as_ref().unwrap();
    assert_eq!(num_pr.num_id.as_ref().unwrap().val, Some(outline));
}

// ----- document level ---------------------------------------------------------------------

#[test]
fn custom_and_application_properties() {
    let mut doc = Document::new();
    doc.add_paragraph("Two words.");
    doc.add_paragraph("And three more.");
    doc.set_custom_property("Client", PropertyValue::Text("ACME & Co".into()))
        .unwrap();
    doc.set_custom_property("Version", PropertyValue::Integer(3))
        .unwrap();
    doc.set_custom_property("Big", PropertyValue::Integer(10_000_000_000))
        .unwrap();
    doc.set_custom_property("Ratio", PropertyValue::Number(0.75))
        .unwrap();
    doc.set_custom_property("Final", PropertyValue::Bool(false))
        .unwrap();
    doc.set_custom_property("Due", PropertyValue::DateTime("2024-12-31T00:00:00Z".into()))
        .unwrap();
    doc.set_custom_property("Version", PropertyValue::Integer(4))
        .unwrap();
    assert!(doc.set_custom_property("", PropertyValue::Bool(true)).is_err());
    let mut app = doc.app_properties().unwrap();
    app.company = Some("ACME".into());
    app.manager = Some("Ann".into());
    doc.set_app_properties(&app).unwrap();
    doc.update_statistics().unwrap();

    let back = save_validate_reopen(&mut doc);
    let props = back.custom_properties().unwrap();
    assert_eq!(props.len(), 6);
    assert_eq!(
        back.custom_property("Version").unwrap(),
        Some(PropertyValue::Integer(4))
    );
    assert_eq!(
        back.custom_property("Big").unwrap(),
        Some(PropertyValue::Integer(10_000_000_000))
    );
    assert_eq!(
        back.custom_property("Client").unwrap(),
        Some(PropertyValue::Text("ACME & Co".into()))
    );
    assert_eq!(
        back.custom_property("Final").unwrap(),
        Some(PropertyValue::Bool(false))
    );
    assert_eq!(back.custom_property("Nope").unwrap(), None);
    let pkg = back.package();
    assert_eq!(
        pkg.related_part(None, rel_types::CUSTOM_PROPERTIES),
        Some(pn("/docProps/custom.xml"))
    );
    assert_eq!(
        pkg.part(&pn("/docProps/custom.xml")).unwrap().content_type(),
        ct::CUSTOM_PROPERTIES
    );
    let xml = part_xml(pkg, "/docProps/custom.xml");
    assert!(
        xml.contains(r#"fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2""#),
        "{xml}"
    );
    for vt in [
        "<vt:lpwstr>",
        "<vt:i4>4</vt:i4>",
        "<vt:i8>",
        "<vt:r8>",
        "<vt:bool>",
        "<vt:filetime>",
    ] {
        assert!(xml.contains(vt), "{vt}");
    }
    let app = back.app_properties().unwrap();
    assert_eq!(app.company.as_deref(), Some("ACME"));
    assert_eq!(app.words, Some(5));
    assert_eq!(app.paragraphs, Some(2));
    assert_eq!(app.characters, Some(22));

    let mut edit = back;
    assert!(edit.remove_custom_property("Ratio").unwrap());
    assert!(!edit.remove_custom_property("Ratio").unwrap());
    let back = save_validate_reopen(&mut edit);
    assert_eq!(back.custom_properties().unwrap().len(), 5);
}

#[test]
fn settings_protection_and_background() {
    let mut doc = Document::new();
    doc.add_paragraph("Protected");
    doc.set_zoom(125).unwrap();
    assert!(doc.set_zoom(5).is_err());
    doc.set_default_tab_stop(Length::cm(1.25)).unwrap();
    doc.set_compatibility_mode(15).unwrap();
    doc.set_background_color("FFFBEA").unwrap();
    doc.protect(Protection::Forms, Some("s3cret")).unwrap();

    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.zoom(), Some(125));
    assert_eq!(
        back.default_tab_stop(),
        Some(Length::twips(Length::cm(1.25).as_twips()))
    );
    assert_eq!(back.compatibility_mode(), Some(15));
    assert_eq!(back.background_color().as_deref(), Some("FFFBEA"));
    assert!(back.displays_background());
    assert_eq!(back.protection(), Some(Protection::Forms));
    assert!(back.check_protection_password("s3cret"));
    assert!(!back.check_protection_password("S3cret"));
    let settings = part_xml(back.package(), "/word/settings.xml");
    for attr in [
        r#"w:edit="forms""#,
        r#"w:cryptProviderType="rsaFull""#,
        r#"w:cryptAlgorithmClass="hash""#,
        r#"w:cryptAlgorithmType="typeAny""#,
        r#"w:cryptAlgorithmSid="4""#,
        r#"w:cryptSpinCount="100000""#,
    ] {
        assert!(settings.contains(attr), "{attr}: {settings}");
    }
    let compat = settings.matches("compatibilityMode").count();
    assert_eq!(compat, 1);

    for kind in [
        Protection::ReadOnly,
        Protection::Comments,
        Protection::TrackedChanges,
    ] {
        let mut d = Document::new();
        d.protect(kind, None).unwrap();
        let back = save_validate_reopen(&mut d);
        assert_eq!(back.protection(), Some(kind));
        assert!(back.check_protection_password("anything"));
        let mut back = back;
        back.unprotect().unwrap();
        assert_eq!(save_validate_reopen(&mut back).protection(), None);
    }
}

// ----- hyperlinks -------------------------------------------------------------------------

#[test]
fn hyperlinks_with_tooltips_mail_and_bookmarks() {
    let mut doc = Document::new();
    doc.add_paragraph("Top");
    doc.add_bookmark(0, TextSpan::Paragraph, "top").unwrap();
    let mut p = doc.add_paragraph("");
    p.add_link(
        "site",
        &LinkTarget::Url("https://example.com/a b".into()),
        Some("Open the site"),
    )
    .unwrap();
    p.add_text(" · ");
    p.add_link(
        "mail",
        &LinkTarget::Email {
            address: "info@example.com".into(),
            subject: Some("Q&A 2024".into()),
        },
        None,
    )
    .unwrap();
    p.add_text(" · ");
    p.add_link("back to top", &LinkTarget::Bookmark("top".into()), Some("Jump"))
        .unwrap();

    let back = save_validate_reopen(&mut doc);
    let links = back.paragraphs()[1].hyperlinks();
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].tooltip.as_deref(), Some("Open the site"));
    assert_eq!(
        back.hyperlink_target(links[0].relationship_id.as_deref().unwrap()),
        Some("https://example.com/a b")
    );
    assert_eq!(
        back.hyperlink_target(links[1].relationship_id.as_deref().unwrap()),
        Some("mailto:info@example.com?subject=Q%26A%202024")
    );
    assert_eq!(
        (links[2].anchor.as_deref(), links[2].tooltip.as_deref()),
        (Some("top"), Some("Jump"))
    );
    assert!(
        back.paragraphs()[1]
            .runs()
            .iter()
            .all(|r| r.style_id() == Some("Hyperlink") || r.text() == " · ")
    );
}

// ----- cross-check with another consumer -------------------------------------------------

/// Builds a document using most features.
fn showcase() -> Document {
    let mut doc = Document::new();
    doc.add_heading("Showcase", 1).unwrap();
    let mut p = doc.add_paragraph("Footnoted claim");
    p.add_footnote("A footnote").unwrap();
    doc.add_comment(1, TextSpan::Text("claim"), &NewComment::new("Ann", "Why?"))
        .unwrap();
    let mut p = doc.add_paragraph("Kept ");
    p.add_insertion("inserted ", &RevisionInfo::new("Bob"));
    p.add_deletion("deleted ", &RevisionInfo::new("Bob"));
    p.add_text("text");
    doc.add_paragraph("Page ").add_field(&Field::Page, "1");
    let control = ContentControl::new(ContentControlKind::PlainText { multi_line: false }, "who");
    doc.add_paragraph("Name: ")
        .add_content_control(&control, "Ann")
        .unwrap();
    doc.add_paragraph("Box: ")
        .add_text_box(
            &ShapeOptions::new(Length::inches(2.0), Length::inches(0.5)),
            |c| {
                c.add_paragraph("boxed");
                Ok(())
            },
        )
        .unwrap();
    doc.add_equation(&Math::frac(Math::text("a"), Math::text("b")));
    doc.add_section(SectionBreak::Continuous).set_columns(&Columns {
        count: 2,
        spacing: Length::cm(1.0),
        separator: false,
    });
    doc.add_paragraph("Last section text");
    doc
}

#[test]
fn showcase_is_valid_and_readable_by_textutil() {
    let mut doc = showcase();
    let back = save_validate_reopen(&mut doc);
    assert!(back.text().contains("Kept inserted text"));
    let textutil = std::path::Path::new("/usr/bin/textutil");
    if !textutil.exists() {
        eprintln!("note: textutil not available, cross-check skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("openxml-docx-showcase-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("showcase.docx");
    doc.save(&path).unwrap();
    let out = std::process::Command::new(textutil)
        .args(["-convert", "txt", "-stdout"])
        .arg(&path)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    for expected in [
        "Showcase",
        "Footnoted claim",
        "Kept inserted",
        "Page 1",
        "Name: Ann",
        "Last section text",
    ] {
        assert!(
            text.contains(expected),
            "{expected:?} missing from textutil output:\n{text}"
        );
    }
    assert!(!text.contains("deleted"), "{text}");
}
