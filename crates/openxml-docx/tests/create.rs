//! Creating documents: every feature is written, saved, reopened, checked
//! and validated against the ECMA-376 schemas.

use openxml_core::image::tiny_png;
use openxml_docx::{
    Alignment, Block, BreakKind, CellAlign, CoreProperties, Document, Error, FontSize, HeaderFooterKind,
    HighlightColor, Length, ListKind, Margins, Orientation, PageSetup, UnderlineStyle, VerticalAlign,
};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{Package, PartName};
use openxml_schema::wml;
use openxml_testkit::validate_package;

/// Saves, validates every part against the schemas and reopens.
fn save_validate_reopen(doc: &mut Document) -> Document {
    let bytes = doc.to_bytes().expect("save");
    let pkg = Package::from_bytes(&bytes).expect("valid package");
    let failures = validate_package(&pkg);
    assert!(failures.is_empty(), "schema validation failed: {failures:#?}");
    Document::from_bytes(&bytes).expect("reopen")
}

fn pn(s: &str) -> PartName {
    PartName::new(s).unwrap()
}

#[test]
fn new_document_has_the_parts_word_expects() {
    let mut doc = Document::new();
    let back = save_validate_reopen(&mut doc);
    let pkg = back.package();
    let main = pkg.main_part().unwrap();
    assert_eq!(main, pn("/word/document.xml"));
    assert_eq!(pkg.part(&main).unwrap().content_type(), ct::WML_DOCUMENT);
    for (rel, name, ty) in [
        (rel_types::STYLES, "/word/styles.xml", ct::WML_STYLES),
        (rel_types::SETTINGS, "/word/settings.xml", ct::WML_SETTINGS),
        (rel_types::FONT_TABLE, "/word/fontTable.xml", ct::WML_FONT_TABLE),
    ] {
        assert_eq!(pkg.related_part(Some(&main), rel), Some(pn(name)), "{rel}");
        assert_eq!(pkg.part(&pn(name)).unwrap().content_type(), ty);
    }
    assert_eq!(
        pkg.related_part(None, rel_types::EXTENDED_PROPERTIES),
        Some(pn("/docProps/app.xml"))
    );
    let core = back.core_properties().unwrap();
    assert!(core.created.is_some() && core.modified.is_some());
    assert_eq!(back.text(), "");
    assert!(back.paragraphs().is_empty());
    assert_eq!(back.page_setup(), PageSetup::a4());
    for id in [
        "Normal",
        "Heading1",
        "Heading6",
        "Title",
        "Subtitle",
        "Quote",
        "ListParagraph",
        "TableGrid",
        "Hyperlink",
    ] {
        assert!(back.style_ids().contains(&id), "missing style {id}");
    }
    assert!(back.headers_and_footers().is_empty());
    assert!(back.header().is_none() && back.footer().is_none());
    assert!(back.numbering().is_none());
    assert_eq!(Document::default().text(), "");
}

#[test]
fn saving_is_deterministic_and_unmodified_documents_round_trip_byte_for_byte() {
    let mut doc = Document::new();
    doc.add_paragraph("stable");
    let first = doc.to_bytes().unwrap();
    let second = doc.to_bytes().unwrap();
    assert_eq!(first, second);
    let mut reopened = Document::from_bytes(&first).unwrap();
    assert_eq!(
        reopened.to_bytes().unwrap(),
        first,
        "nothing changed, nothing rewritten"
    );
}

#[test]
fn run_formatting_round_trips() {
    let mut doc = Document::new();
    let mut p = doc.add_paragraph("");
    p.add_run("bold").bold(true);
    p.add_run("italic").italic(true);
    p.add_run("under").underline(Some(UnderlineStyle::Double));
    p.add_run("strike").strike(true);
    p.add_run("big").size(FontSize(16.5));
    p.add_run("red").color("#C00000").unwrap();
    p.add_run("mono").font("Courier New");
    p.add_run("mark").highlight(Some(HighlightColor::Yellow));
    p.add_run("x2").vertical_align(VerticalAlign::Superscript);
    p.add_run("caps").small_caps(true).style("Hyperlink");
    let back = save_validate_reopen(&mut doc);
    let runs = back.paragraphs()[0].runs();
    assert_eq!(runs.len(), 10);
    assert!(runs[0].is_bold() && !runs[0].is_italic());
    assert!(runs[1].is_italic());
    assert_eq!(runs[2].underline(), Some(UnderlineStyle::Double));
    assert!(runs[3].is_strike());
    assert_eq!(runs[4].size(), Some(FontSize(16.5)));
    assert_eq!(runs[5].color().as_deref(), Some("C00000"));
    assert_eq!(runs[6].font(), Some("Courier New"));
    assert_eq!(runs[7].highlight(), Some(HighlightColor::Yellow));
    assert_eq!(runs[8].vertical_align(), Some(VerticalAlign::Superscript));
    assert_eq!(runs[9].style_id(), Some("Hyperlink"));
    assert_eq!(back.text(), "bolditalicunderstrikebigredmonomarkx2caps");
}

#[test]
fn invalid_color_is_rejected() {
    let mut doc = Document::new();
    let mut p = doc.add_paragraph("");
    let err = p.add_run("x").color("purple").unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)), "{err}");
}

#[test]
fn headings_use_builtin_styles() {
    let mut doc = Document::new();
    doc.add_heading("Title", 0).unwrap();
    for level in 1..=6u8 {
        doc.add_heading(&format!("H{level}"), level).unwrap();
    }
    assert!(matches!(
        doc.add_heading("bad", 7),
        Err(Error::InvalidArgument(_))
    ));
    let back = save_validate_reopen(&mut doc);
    let styles: Vec<_> = back
        .paragraphs()
        .iter()
        .map(|p| p.style_id().unwrap().to_owned())
        .collect();
    assert_eq!(
        styles,
        [
            "Title", "Heading1", "Heading2", "Heading3", "Heading4", "Heading5", "Heading6"
        ]
    );
}

#[test]
fn paragraph_properties_round_trip() {
    let mut doc = Document::new();
    let mut p = doc.add_paragraph("centered");
    p.set_alignment(Alignment::Center)
        .set_spacing(Some(Length::pt(6.0)), Some(Length::pt(12.0)))
        .set_indent(Length::inches(0.5), Length::inches(-0.25))
        .set_keep_with_next(true)
        .set_page_break_before(true);
    doc.add_paragraph("justified").set_alignment(Alignment::Justify);
    doc.add_paragraph("right")
        .set_alignment(Alignment::Right)
        .set_indent(Length::ZERO, Length::pt(18.0));
    doc.add_paragraph("styled")
        .set_style("Quote")
        .unwrap()
        .clear_style();
    let back = save_validate_reopen(&mut doc);
    let ps = back.paragraphs();
    assert_eq!(ps[0].alignment(), Some(Alignment::Center));
    assert!(ps[0].page_break_before());
    let ppr = ps[0].raw().p_pr.as_ref().unwrap();
    let spacing = ppr.spacing.as_ref().unwrap();
    assert_eq!(
        spacing.before,
        Some(openxml_schema::shared_types::ST_TwipsMeasure::UnsignedDecimalNumber(120))
    );
    assert_eq!(
        spacing.after,
        Some(openxml_schema::shared_types::ST_TwipsMeasure::UnsignedDecimalNumber(240))
    );
    let ind = ppr.ind.as_ref().unwrap();
    assert_eq!(ind.left, Some(wml::ST_SignedTwipsMeasure::Integer(720)));
    assert_eq!(
        ind.hanging,
        Some(openxml_schema::shared_types::ST_TwipsMeasure::UnsignedDecimalNumber(360))
    );
    assert!(ppr.keep_next.is_some());
    assert_eq!(ps[1].alignment(), Some(Alignment::Justify));
    assert_eq!(ps[2].alignment(), Some(Alignment::Right));
    let ind2 = ps[2].raw().p_pr.as_ref().unwrap().ind.as_ref().unwrap();
    assert_eq!(
        ind2.first_line,
        Some(openxml_schema::shared_types::ST_TwipsMeasure::UnsignedDecimalNumber(360))
    );
    assert_eq!(ps[3].style_id(), None);
}

#[test]
fn hyperlinks_create_external_relationships() {
    let mut doc = Document::new();
    let mut p = doc.add_paragraph("See ");
    p.add_hyperlink(
        "the spec",
        "https://ecma-international.org/publications-and-standards/standards/ecma-376/",
    )
    .unwrap()
    .add_text(" and ")
    .add_internal_hyperlink("chapter 2", "_Toc2")
    .unwrap();
    let back = save_validate_reopen(&mut doc);
    let para = back.paragraphs()[0];
    assert_eq!(para.text(), "See the spec and chapter 2");
    let links = para.hyperlinks();
    assert_eq!(links.len(), 2);
    let target = back
        .hyperlink_target(links[0].relationship_id.as_deref().unwrap())
        .unwrap();
    assert!(target.starts_with("https://ecma-international.org/"));
    assert_eq!(links[1].anchor.as_deref(), Some("_Toc2"));
    assert!(back.hyperlink_target("rId999").is_none());
    let rels = back.package().relationships(Some(back.main_part_name())).unwrap();
    assert!(rels.by_type(rel_types::HYPERLINK).all(|r| r.is_external()));
    assert_eq!(para.runs()[1].style_id(), Some("Hyperlink"));
}

#[test]
fn tabs_and_breaks_appear_in_text() {
    let mut doc = Document::new();
    let mut p = doc.add_paragraph("a");
    p.add_tab().add_text("b").add_break(BreakKind::Line).add_text("c");
    p.add_run("d").add_break(BreakKind::Column);
    doc.add_page_break();
    doc.add_paragraph("after the break");
    let back = save_validate_reopen(&mut doc);
    // Paragraph 1: "a" TAB "b" LF "c" "d" LF(column break); paragraph 2: LF(page break).
    assert_eq!(back.text(), "a\tb\ncd\n\n\n\nafter the break");
    let breaks: Vec<_> = back.paragraphs()[1]
        .raw()
        .p_content
        .iter()
        .flat_map(|c| match c {
            wml::EG_PContent::R(r) => r.run_inner_content.clone(),
            _ => vec![],
        })
        .collect();
    assert!(matches!(&breaks[0], wml::EG_RunInnerContent::Br(b) if b.type_ == Some(wml::ST_BrType::Page)));
}

#[test]
fn tables_with_widths_merges_and_formatting() {
    let mut doc = Document::new();
    {
        let mut t = doc.add_table(3, 3).unwrap();
        assert_eq!((t.row_count(), t.column_count()), (3, 3));
        t.cell(0, 0).unwrap().set_text("Name").add_run(" (id)").bold(true);
        t.cell(0, 1).unwrap().set_text("Qty");
        t.cell(0, 2).unwrap().set_text("Note");
        t.cell(1, 0).unwrap().set_text("apple");
        t.cell(1, 1).unwrap().set_text("3");
        t.cell(1, 2).unwrap().set_text("x");
        t.cell(2, 0).unwrap().set_text("pear");
        {
            let mut c = t.cell(2, 1).unwrap();
            c.add_paragraph("line 1");
            c.add_paragraph("line 2");
            c.set_shading("FFF2CC")
                .unwrap()
                .set_width(Length::inches(1.0))
                .set_vertical_alignment(CellAlign::Center);
        }
        t.set_header_row(0, true).unwrap();
        t.set_column_widths(&[Length::inches(2.0), Length::inches(1.5), Length::inches(3.0)])
            .unwrap();
        assert!(t.set_column_widths(&[Length::inches(1.0)]).is_err());
        let new_row = t.add_row();
        assert_eq!(new_row, 3);
        t.cell(3, 0).unwrap().set_text("merged a");
        t.cell(3, 1).unwrap().set_text("merged b");
        t.merge_horizontally(3, 0, 1).unwrap();
        t.merge_vertically(2, 1, 2).unwrap();
        assert!(t.cell(9, 0).is_err());
        assert!(t.cell(0, 9).is_err());
        assert!(t.merge_horizontally(0, 1, 1).is_err());
        assert!(t.merge_vertically(0, 0, 9).is_err());
        assert!(t.set_header_row(9, true).is_err());
        t.set_style("TableGrid").unwrap();
        assert_eq!(t.view().style_id(), Some("TableGrid"));
    }
    assert!(matches!(doc.add_table(0, 2), Err(Error::InvalidArgument(_))));
    let back = save_validate_reopen(&mut doc);
    let t = back.tables()[0];
    assert_eq!(t.row_count(), 4);
    assert_eq!(t.cell(0, 0).unwrap().text(), "Name (id)");
    assert!(t.rows()[0].is_header());
    assert_eq!(t.cell(2, 1).unwrap().text(), "line 1\nline 2");
    let row3 = &t.rows()[3];
    assert_eq!(row3.cells().len(), 2, "two cells merged into one");
    assert_eq!(row3.cells()[0].grid_span(), 2);
    assert_eq!(row3.cells()[0].text(), "merged a\nmerged b");
    assert_eq!(t.cell(1, 2).unwrap().vertical_merge(), Some(true));
    assert_eq!(t.cell(2, 2).unwrap().vertical_merge(), Some(false));
    assert_eq!(t.cell(1, 2).unwrap().text(), "x");
    let grid = t.raw().tbl_grid.as_ref().unwrap();
    assert_eq!(
        grid.grid_col[2].w,
        Some(openxml_schema::shared_types::ST_TwipsMeasure::UnsignedDecimalNumber(4320))
    );
    assert_eq!(
        back.text(),
        "Name (id)\tQty\tNote\napple\t3\tx\npear\tline 1\nline 2\t\nmerged a\nmerged b\t"
    );
    let mut back = back;
    let mut tm = back.table_mut(0).unwrap();
    tm.cell(1, 0).unwrap().set_text("banana");
    assert!(back.table_mut(1).is_none());
    assert_eq!(back.tables()[0].cell(1, 0).unwrap().text(), "banana");
}

#[test]
fn pictures_are_embedded_with_unique_ids() {
    let png = tiny_png(200, 100);
    let mut doc = Document::new();
    doc.add_picture(&png, Length::inches(2.0)).unwrap();
    doc.add_paragraph("caption: ")
        .add_picture(&png, Length::inches(1.0))
        .unwrap();
    assert!(matches!(
        doc.add_picture(b"not an image", Length::inches(1.0)),
        Err(Error::UnsupportedImage)
    ));
    assert!(matches!(
        doc.add_picture(&png, Length::ZERO),
        Err(Error::InvalidArgument(_))
    ));
    let back = save_validate_reopen(&mut doc);
    let main = back.main_part_name().clone();
    let images = back.package().related_parts(Some(&main), rel_types::IMAGE);
    assert_eq!(
        images,
        [pn("/word/media/image1.png"), pn("/word/media/image2.png")]
    );
    assert_eq!(
        back.package().part(&images[0]).unwrap().content_type(),
        "image/png"
    );
    assert_eq!(back.package().part(&images[0]).unwrap().data(), png.as_slice());
    let mut ids = Vec::new();
    for p in back.paragraphs() {
        for r in p.runs() {
            if !r.has_drawing() {
                continue;
            }
            for c in &r.raw().run_inner_content {
                let wml::EG_RunInnerContent::Drawing(d) = c else {
                    continue;
                };
                let wml::CT_Drawing_Choice::Inline(inline) = &d.choice[0] else {
                    panic!()
                };
                ids.push(inline.doc_pr.as_ref().unwrap().id.unwrap());
                let ext = inline.extent.as_ref().unwrap();
                assert_eq!(ext.cx.unwrap() * 100 / ext.cy.unwrap(), 200, "aspect ratio kept");
            }
        }
    }
    assert_eq!(ids, [1, 2]);
    // Reopening continues the numbering of drawing ids.
    let mut back = back;
    back.add_picture(&png, Length::inches(1.0)).unwrap();
    let again = save_validate_reopen(&mut back);
    let xml = String::from_utf8(again.package().part(&main).unwrap().data().to_vec()).unwrap();
    assert!(xml.contains(r#"id="3""#), "{xml}");
    assert!(!again.paragraphs()[0].is_empty());
}

#[test]
fn bulleted_and_numbered_lists() {
    let mut doc = Document::new();
    doc.add_list_item("one", ListKind::Numbered, 0).unwrap();
    doc.add_list_item("one.a", ListKind::Numbered, 1).unwrap();
    doc.add_list_item("two", ListKind::Numbered, 0).unwrap();
    doc.add_list_item("bullet", ListKind::Bullet, 0).unwrap();
    doc.restart_list(ListKind::Numbered).unwrap();
    doc.restart_list(ListKind::Numbered).unwrap();
    doc.add_list_item("again one", ListKind::Numbered, 0).unwrap();
    assert!(matches!(
        doc.add_list_item("deep", ListKind::Bullet, 9),
        Err(Error::InvalidArgument(_))
    ));
    let mut fresh = Document::new();
    fresh.restart_list(ListKind::Bullet).unwrap();
    assert!(
        fresh.numbering().is_none(),
        "restarting before any item is a no-op"
    );
    let back = save_validate_reopen(&mut doc);
    let main = back.main_part_name().clone();
    assert_eq!(
        back.package().related_part(Some(&main), rel_types::NUMBERING),
        Some(pn("/word/numbering.xml"))
    );
    let ps = back.paragraphs();
    let nums: Vec<_> = ps.iter().map(|p| p.numbering().unwrap()).collect();
    assert_eq!(nums[0].0, nums[1].0);
    assert_eq!(nums[1].1, 1);
    assert_eq!(nums[0].0, nums[2].0);
    assert_ne!(nums[3].0, nums[0].0, "bullets use their own definition");
    assert_ne!(nums[4].0, nums[0].0, "restarted list");
    assert!(ps.iter().all(|p| p.style_id() == Some("ListParagraph")));
    let numbering = back.numbering().unwrap();
    assert_eq!(numbering.abstract_num.len(), 2);
    let restarted = numbering
        .num
        .iter()
        .find(|n| n.num_id == Some(nums[4].0))
        .unwrap();
    assert_eq!(
        restarted.lvl_override[0].start_override.as_ref().unwrap().val,
        Some(1)
    );
}

#[test]
fn headers_and_footers() {
    let mut doc = Document::new();
    doc.set_header("Company confidential")
        .unwrap()
        .set_alignment(Alignment::Right);
    doc.set_footer("Page footer")
        .unwrap()
        .add_hyperlink(" (site)", "https://example.org")
        .unwrap();
    doc.add_paragraph("body");
    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.header().unwrap().text(), "Company confidential");
    assert_eq!(back.header().unwrap().kind(), HeaderFooterKind::Header);
    assert_eq!(back.footer().unwrap().text(), "Page footer (site)");
    assert_eq!(back.footer().unwrap().paragraphs()[0].style_id(), Some("Footer"));
    assert_eq!(back.header().unwrap().part_name(), &pn("/word/header1.xml"));
    let footer_part = back.footer().unwrap().part_name().clone();
    let link = back
        .package()
        .relationships(Some(&footer_part))
        .unwrap()
        .first_by_type(rel_types::HYPERLINK)
        .unwrap();
    assert!(
        link.is_external(),
        "hyperlink relationships belong to the footer part"
    );
    assert_eq!(back.headers_and_footers().len(), 2);
    assert!(!back.header().unwrap().raw().block_level_elts.is_empty());
    // Setting the header again replaces the content of the same part.
    let mut back = back;
    back.set_header("Replaced").unwrap();
    let again = save_validate_reopen(&mut back);
    assert_eq!(again.header().unwrap().text(), "Replaced");
    assert_eq!(again.headers_and_footers().len(), 2);
    let sect = again.body().sect_pr.as_ref().unwrap();
    assert_eq!(sect.hdr_ftr_references.len(), 2);
}

#[test]
fn page_setup_round_trips() {
    let mut doc = Document::new();
    let setup = PageSetup {
        margins: Margins {
            left: Length::cm(3.0),
            right: Length::cm(2.0),
            ..Margins::default()
        },
        ..PageSetup::letter().landscape()
    };
    doc.set_page_setup(&setup);
    let back = save_validate_reopen(&mut doc);
    let read = back.page_setup();
    assert_eq!(read.orientation, Orientation::Landscape);
    assert_eq!(read.width.as_twips(), 15840);
    assert_eq!(read.height.as_twips(), 12240);
    assert_eq!(read.margins.left.as_twips(), Length::cm(3.0).as_twips());
    assert_eq!(read.margins.right.as_twips(), Length::cm(2.0).as_twips());
}

#[test]
fn replace_text_across_runs_tables_and_headers() {
    let mut doc = Document::new();
    let mut p = doc.add_paragraph("Hello ");
    p.add_run("{{na").bold(true);
    p.add_run("me}}!");
    let mut t = doc.add_table(1, 1).unwrap();
    t.cell(0, 0).unwrap().set_text("Dear {{name}}");
    doc.set_header("For {{name}}").unwrap();
    let count = doc.replace_text("{{name}}", "Ada");
    assert_eq!(count, 3);
    assert_eq!(doc.replace_text("", "x"), 0);
    assert_eq!(doc.paragraph_mut(0).unwrap().replace_text("Ada", "Grace"), 1);
    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.paragraphs()[0].text(), "Hello Grace!");
    assert!(
        back.paragraphs()[0].runs()[1].is_bold(),
        "replacement takes the formatting where the match starts"
    );
    assert_eq!(back.tables()[0].text(), "Dear Ada");
    assert_eq!(back.header().unwrap().text(), "For Ada");
}

#[test]
fn editing_and_removing_paragraphs() {
    let mut doc = Document::new();
    for i in 0..4 {
        doc.add_paragraph(&format!("p{i}"));
    }
    doc.remove_paragraph(1).unwrap();
    assert!(matches!(doc.remove_paragraph(10), Err(Error::NotFound(_))));
    doc.paragraph_mut(0).unwrap().clear().add_text("first");
    doc.paragraph_mut(2).unwrap().run_mut(0).unwrap().italic(true);
    assert!(doc.paragraph_mut(3).is_none());
    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.text(), "first\np2\np3");
    assert!(back.paragraphs()[2].runs()[0].is_italic());
    assert!(matches!(back.blocks()[0], Block::Paragraph(_)));
}

#[test]
fn core_properties_round_trip() {
    let mut doc = Document::new();
    let props = CoreProperties {
        title: Some("Báo cáo".into()),
        creator: Some("Nguyễn Văn A".into()),
        keywords: Some("rust, docx".into()),
        ..doc.core_properties().unwrap()
    };
    doc.set_core_properties(&props).unwrap();
    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.core_properties().unwrap(), props);
}

#[test]
fn unicode_and_significant_whitespace_survive() {
    let mut doc = Document::new();
    doc.add_paragraph("  Tiếng Việt có dấu — 中文 — עברית — 🦀  ");
    doc.add_paragraph("tab\tinside");
    doc.add_paragraph("A & B < C > D \"quoted\"");
    let bytes = doc.to_bytes().unwrap();
    let xml = String::from_utf8(
        Package::from_bytes(&bytes)
            .unwrap()
            .part(&pn("/word/document.xml"))
            .unwrap()
            .data()
            .to_vec(),
    )
    .unwrap();
    assert!(xml.contains(r#"<w:t xml:space="preserve">  Tiếng Việt"#), "{xml}");
    let back = save_validate_reopen(&mut doc);
    assert_eq!(
        back.text(),
        "  Tiếng Việt có dấu — 中文 — עברית — 🦀  \ntab\tinside\nA & B < C > D \"quoted\""
    );
}

#[test]
fn builtin_styles_are_added_to_documents_that_lack_them() {
    // A minimal document without a styles part.
    let mut pkg = Package::new();
    let main = pn("/word/document.xml");
    let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>"#;
    pkg.add_part(main.clone(), ct::WML_DOCUMENT, xml.as_bytes().to_vec())
        .unwrap();
    pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &main)
        .unwrap();
    let mut doc = Document::from_package(pkg).unwrap();
    assert!(doc.styles().is_none());
    doc.add_heading("Added", 2).unwrap();
    doc.add_paragraph("")
        .add_hyperlink("link", "https://example.com")
        .unwrap();
    let back = save_validate_reopen(&mut doc);
    let ids = back.style_ids();
    for id in ["Heading2", "Normal", "Hyperlink", "DefaultParagraphFont"] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    assert_eq!(
        back.page_setup(),
        PageSetup::letter(),
        "no section properties: Word defaults"
    );
}

#[test]
fn localized_style_names_are_reused() {
    let mut doc = Document::new();
    // Simulate a localized document: Heading1 exists under another id.
    for s in &mut doc.styles_mut().unwrap().style {
        if s.style_id.as_deref() == Some("Heading1") {
            s.style_id = Some("berschrift1".into());
        }
    }
    doc.add_heading("Kapitel", 1).unwrap();
    assert_eq!(doc.paragraphs()[0].style_id(), Some("berschrift1"));
    assert!(!doc.style_ids().contains(&"Heading1"));
    // Unknown style ids are used as given.
    doc.add_paragraph("custom").set_style("MyStyle").unwrap();
    assert_eq!(doc.paragraphs()[1].style_id(), Some("MyStyle"));
}

fn document_from_body(body: &str) -> Document {
    let mut pkg = Package::new();
    let main = pn("/word/document.xml");
    let xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    );
    pkg.add_part(main.clone(), ct::WML_DOCUMENT, xml.into_bytes())
        .unwrap();
    pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &main)
        .unwrap();
    Document::from_package(pkg).unwrap()
}

#[test]
fn content_controls_are_looked_through_for_reading_and_editing() {
    let mut doc = document_from_body(
        r#"<w:p><w:r><w:t>before</w:t></w:r></w:p>
           <w:sdt><w:sdtContent>
             <w:p><w:r><w:t>inside 1</w:t></w:r></w:p>
             <w:customXml w:element="x"><w:p><w:r><w:t>inside 2</w:t></w:r></w:p></w:customXml>
             <w:tbl><w:tblGrid><w:gridCol w:w="100"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
           </w:sdtContent></w:sdt>
           <w:p><w:r><w:t>after</w:t></w:r></w:p>"#,
    );
    assert_eq!(doc.paragraphs().len(), 4);
    assert_eq!(doc.tables().len(), 1);
    assert_eq!(doc.text(), "before\ninside 1\ninside 2\ncell\nafter");
    doc.paragraph_mut(2).unwrap().add_text("!");
    doc.table_mut(0).unwrap().cell(0, 0).unwrap().set_text("changed");
    doc.remove_paragraph(1).unwrap();
    assert_eq!(doc.text(), "before\ninside 2!\nchanged\nafter");
    doc.remove_paragraph(1).unwrap();
    doc.remove_paragraph(1).unwrap();
    assert_eq!(doc.text(), "before\nchanged");
    assert!(doc.remove_paragraph(1).is_err());
    let bytes = doc.to_bytes().unwrap();
    assert_eq!(Document::from_bytes(&bytes).unwrap().text(), "before\nchanged");
}

#[test]
fn opening_non_word_packages_fails_cleanly() {
    let mut pkg = Package::new();
    let main = pn("/xl/workbook.xml");
    pkg.add_part(main.clone(), ct::SML_WORKBOOK, b"<workbook/>".to_vec())
        .unwrap();
    pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &main)
        .unwrap();
    assert!(matches!(
        Document::from_package(pkg),
        Err(Error::InvalidDocument(_))
    ));
    assert!(matches!(
        Document::from_package(Package::new()),
        Err(Error::InvalidDocument(_))
    ));
    assert!(Document::from_bytes(b"garbage").is_err());
    assert!(Document::open("/nonexistent/file.docx").is_err());
    let mut bad = Package::new();
    let main = pn("/word/document.xml");
    bad.add_part(main.clone(), ct::WML_DOCUMENT, b"<w:document".to_vec())
        .unwrap();
    bad.add_relationship(None, rel_types::OFFICE_DOCUMENT, &main)
        .unwrap();
    let err = Document::from_package(bad).unwrap_err();
    assert!(err.to_string().contains("/word/document.xml"), "{err}");
}

#[test]
fn file_and_writer_apis() {
    let dir = std::env::temp_dir().join(format!("openxml-docx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hello.docx");
    let mut doc = Document::new();
    doc.add_paragraph("file");
    doc.save(&path).unwrap();
    let back = Document::open(&path).unwrap();
    assert_eq!(back.text(), "file");
    let reader = std::fs::File::open(&path).unwrap();
    assert_eq!(Document::from_reader(reader).unwrap().text(), "file");
    let cursor = doc.write_to(std::io::Cursor::new(Vec::new())).unwrap();
    assert_eq!(Document::from_bytes(&cursor.into_inner()).unwrap().text(), "file");
    let pkg = doc.into_package().unwrap();
    assert!(pkg.main_part().is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn raw_escape_hatches_mark_parts_modified() {
    let mut doc = Document::new();
    doc.flush().unwrap();
    let before = doc
        .package()
        .part(&pn("/word/document.xml"))
        .unwrap()
        .data()
        .to_vec();
    doc.body_mut()
        .block_level_elts
        .push(wml::EG_BlockLevelElts::P(Box::default()));
    doc.document_mut()
        .body
        .as_mut()
        .unwrap()
        .block_level_elts
        .push(wml::EG_BlockLevelElts::P(Box::default()));
    doc.flush().unwrap();
    let after = doc
        .package()
        .part(&pn("/word/document.xml"))
        .unwrap()
        .data()
        .to_vec();
    assert_ne!(before, after);
    assert_eq!(doc.paragraphs().len(), 2);
    assert_eq!(doc.document().body.as_ref().unwrap().block_level_elts.len(), 2);
    doc.package_mut();
}
