//! Behaviour of the generated schema types.

use openxml_schema::shared_types::{ST_OnOff, ST_OnOff1, ST_TwipsMeasure};
use openxml_schema::wml::{self, EG_BlockLevelElts, EG_PContent, EG_RPrBase, EG_RunInnerContent};
use openxml_schema::{dml, pml, sml};
use openxml_testkit::{Validation, validate_xml};
use openxml_xml::{Ns, RawElement, XmlValue};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn doc(body: &str) -> String {
    format!(r#"<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#)
}

fn first_paragraph(d: &wml::CT_Document) -> &wml::CT_P {
    match &d.body.as_ref().unwrap().block_level_elts[0] {
        EG_BlockLevelElts::P(p) => p,
        other => panic!("{other:?}"),
    }
}

#[test]
fn reads_paragraph_runs_and_text() {
    let d = wml::elements::DOCUMENT
        .parse(&doc(
            r#"<w:p w:rsidR="00AB12CD"><w:pPr><w:jc w:val="center"/></w:pPr>
                 <w:r><w:rPr><w:b/><w:sz w:val="28"/></w:rPr><w:t xml:space="preserve"> Hello </w:t></w:r></w:p>"#,
        ))
        .unwrap();
    let p = first_paragraph(&d);
    assert_eq!(p.rsid_r.as_ref().unwrap().to_u64(), Some(0x00AB12CD));
    let jc = p.p_pr.as_ref().unwrap().p_pr_base_jc();
    assert_eq!(jc, Some(wml::ST_Jc::Center));
    let EG_PContent::R(run) = &p.p_content[0] else {
        panic!()
    };
    let props = &run.r_pr.as_ref().unwrap().r_pr_base;
    assert!(matches!(&props[0], EG_RPrBase::B(b) if b.val.is_none()));
    assert!(
        matches!(&props[1], EG_RPrBase::Sz(sz) if sz.val == Some(wml::ST_HpsMeasure::UnsignedDecimalNumber(28)))
    );
    let EG_RunInnerContent::T(t) = &run.run_inner_content[0] else {
        panic!()
    };
    assert_eq!(t.value, " Hello ");
    assert_eq!(t.xml_space.as_deref(), Some("preserve"));
}

/// Small helper implemented on the generated type to keep the test readable.
trait JcExt {
    fn p_pr_base_jc(&self) -> Option<wml::ST_Jc>;
}

impl JcExt for wml::CT_PPr {
    fn p_pr_base_jc(&self) -> Option<wml::ST_Jc> {
        self.jc.as_ref().and_then(|j| j.val)
    }
}

#[test]
fn builds_a_document_in_code_that_validates_against_the_schema() {
    let text = wml::CT_Text {
        value: "Built in Rust".into(),
        xml_space: None,
        ..Default::default()
    };
    let run = wml::CT_R {
        r_pr: Some(Box::new(wml::CT_RPr {
            r_pr_base: vec![
                EG_RPrBase::B(Box::default()),
                EG_RPrBase::Color(Box::new(wml::CT_Color {
                    val: Some(wml::ST_HexColor::HexColorRGB(openxml_xml::HexBinary(vec![
                        0xFF, 0, 0,
                    ]))),
                    ..Default::default()
                })),
            ],
            ..Default::default()
        })),
        run_inner_content: vec![EG_RunInnerContent::T(Box::new(text))],
        ..Default::default()
    };
    let paragraph = wml::CT_P {
        p_content: vec![EG_PContent::R(Box::new(run))],
        ..Default::default()
    };
    let cell = wml::CT_Tc {
        block_level_elts: vec![EG_BlockLevelElts::P(Box::new(paragraph.clone()))],
        ..Default::default()
    };
    let row = wml::CT_Row {
        content_cell_content: vec![wml::EG_ContentCellContent::Tc(Box::new(cell))],
        ..Default::default()
    };
    let table = wml::CT_Tbl {
        tbl_pr: Some(Box::default()),
        tbl_grid: Some(Box::new(wml::CT_TblGrid {
            grid_col: vec![wml::CT_TblGridCol {
                w: Some(ST_TwipsMeasure::UnsignedDecimalNumber(2000)),
                ..Default::default()
            }],
            ..Default::default()
        })),
        content_row_content: vec![wml::EG_ContentRowContent::Tr(Box::new(row))],
        ..Default::default()
    };
    let document = wml::CT_Document {
        body: Some(Box::new(wml::CT_Body {
            block_level_elts: vec![
                EG_BlockLevelElts::P(Box::new(paragraph)),
                EG_BlockLevelElts::Tbl(Box::new(table)),
            ],
            ..Default::default()
        })),
        ..Default::default()
    };
    let xml = wml::elements::DOCUMENT.to_xml(&document);
    assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#));
    assert!(xml.contains(r#"<w:color w:val="FF0000"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:gridCol w:w="2000"/>"#));
    match validate_xml(&xml) {
        Ok(Validation::Valid | Validation::Skipped) => {}
        Err(e) => panic!("schema validation failed:\n{e}\n{xml}"),
    }
    let mut back = wml::elements::DOCUMENT.parse(&xml).unwrap();
    // The root keeps the namespace declarations it was read with.
    assert!(back.extra_attrs.iter().all(|a| a.is_namespace_declaration()));
    back.extra_attrs.clear();
    assert_eq!(back, document);
}

#[test]
fn strict_documents_are_read_and_written_as_transitional() {
    let strict = r#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main" w:conformance="strict"><w:body><w:p><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>"#;
    let d = wml::elements::DOCUMENT.parse(strict).unwrap();
    assert_eq!(
        d.conformance,
        Some(openxml_schema::shared_types::ST_ConformanceClass::Strict)
    );
    let out = wml::elements::DOCUMENT.to_xml(&d);
    assert!(out.contains(&format!(r#"xmlns:w="{W}""#)), "{out}");
    assert!(!out.contains("purl.oclc.org"), "{out}");
}

#[test]
fn markup_compatibility_content_keeps_its_position() {
    let xml = format!(
        r#"<w:document xmlns:w="{W}" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" mc:Ignorable="w14"><w:body><w:p w14:paraId="1A2B3C4D"><w:r><w:rPr><w:b/></w:rPr><mc:AlternateContent><mc:Choice Requires="w14"><w:t>new</w:t></mc:Choice><mc:Fallback><w:t>old</w:t></mc:Fallback></mc:AlternateContent><w:t>tail</w:t></w:r></w:p></w:body></w:document>"#
    );
    let d = wml::elements::DOCUMENT.parse(&xml).unwrap();
    let p = first_paragraph(&d);
    assert!(
        p.extra_attrs
            .iter()
            .any(|a| &*a.name.local == "paraId" && a.value == "1A2B3C4D")
    );
    let EG_PContent::R(run) = &p.p_content[0] else {
        panic!()
    };
    assert_eq!(
        run.extra_children.len(),
        1,
        "AlternateContent between rPr and content"
    );
    assert_eq!(run.extra_children[0].anchor, 1);
    let out = wml::elements::DOCUMENT.to_xml(&d);
    let expected_order = out.find("<w:b/>").unwrap() < out.find("<mc:AlternateContent>").unwrap()
        && out.find("</mc:AlternateContent>").unwrap() < out.find("<w:t>tail</w:t>").unwrap();
    assert!(expected_order, "{out}");
    assert!(
        out.contains(r#"mc:Ignorable="w14""#) && out.contains("xmlns:w14="),
        "{out}"
    );
    let back = RawElement::parse(&out).unwrap();
    let diffs = openxml_xml::compare::semantic_diff(&RawElement::parse(&xml).unwrap(), &back);
    assert!(diffs.is_empty(), "{diffs:?}");
}

#[test]
fn unknown_elements_inside_repeating_choices_stay_in_order() {
    let xml = doc(r#"<w:p><w:r><w:t>a</w:t></w:r><x:ext xmlns:x="urn:ext"/><w:r><w:t>b</w:t></w:r></w:p>"#);
    let d = wml::elements::DOCUMENT.parse(&xml).unwrap();
    let p = first_paragraph(&d);
    assert_eq!(p.p_content.len(), 3);
    assert!(matches!(&p.p_content[1], EG_PContent::Other(raw) if raw.name.uri() == "urn:ext"));
    assert_eq!(p.p_content[1].element_name().1, "ext");
    let out = wml::elements::DOCUMENT.to_xml(&d);
    assert!(out.find("<w:t>a</w:t>").unwrap() < out.find("ext").unwrap());
    assert!(out.find("ext").unwrap() < out.find("<w:t>b</w:t>").unwrap());
}

#[test]
fn out_of_order_children_are_accepted_and_written_in_schema_order() {
    let xml = doc(r#"<w:p><w:r><w:t>x</w:t></w:r><w:pPr><w:jc w:val="right"/></w:pPr></w:p>"#);
    let d = wml::elements::DOCUMENT.parse(&xml).unwrap();
    let p = first_paragraph(&d);
    assert!(p.p_pr.is_some());
    let out = wml::elements::DOCUMENT.to_xml(&d);
    assert!(out.find("<w:pPr>").unwrap() < out.find("<w:r>").unwrap(), "{out}");
}

#[test]
fn duplicate_single_elements_are_preserved_as_extras() {
    let xml = doc(r#"<w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:pPr><w:jc w:val="right"/></w:pPr></w:p>"#);
    let d = wml::elements::DOCUMENT.parse(&xml).unwrap();
    let p = first_paragraph(&d);
    assert_eq!(
        p.p_pr.as_ref().unwrap().jc.as_ref().unwrap().val,
        Some(wml::ST_Jc::Left)
    );
    assert_eq!(p.extra_children.len(), 1);
    let out = wml::elements::DOCUMENT.to_xml(&d);
    assert_eq!(out.matches("<w:pPr>").count(), 2);
}

#[test]
fn invalid_attribute_values_are_kept_verbatim() {
    let xml = doc(r#"<w:p><w:pPr><w:jc w:val="sideways"/></w:pPr></w:p>"#);
    let d = wml::elements::DOCUMENT.parse(&xml).unwrap();
    let jc = first_paragraph(&d)
        .p_pr
        .as_ref()
        .unwrap()
        .jc
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(jc.val, None);
    assert_eq!(jc.extra_attrs[0].value, "sideways");
    let out = wml::elements::DOCUMENT.to_xml(&d);
    assert!(out.contains(r#"<w:jc w:val="sideways"/>"#), "{out}");
}

#[test]
fn simple_type_enums_and_unions() {
    for v in wml::ST_Jc::ALL {
        assert_eq!(wml::ST_Jc::parse_xml(v.as_str()), Some(*v));
        assert_eq!(v.to_string(), v.as_str());
    }
    assert_eq!(wml::ST_Jc::default(), wml::ST_Jc::ALL[0]);
    assert_eq!(wml::ST_Jc::parse_xml("nonsense"), None);
    // Unions try non-string members first.
    assert_eq!(ST_OnOff::parse_xml("1"), Some(ST_OnOff::Boolean(true)));
    assert_eq!(ST_OnOff::parse_xml("off"), Some(ST_OnOff::OnOff1(ST_OnOff1::Off)));
    assert_eq!(ST_OnOff::parse_xml("maybe"), None);
    assert_eq!(
        ST_TwipsMeasure::parse_xml("720"),
        Some(ST_TwipsMeasure::UnsignedDecimalNumber(720))
    );
    assert_eq!(
        ST_TwipsMeasure::parse_xml("1.5in"),
        Some(ST_TwipsMeasure::PositiveUniversalMeasure("1.5in".into()))
    );
    assert_eq!(
        ST_TwipsMeasure::PositiveUniversalMeasure("2cm".into()).to_xml_string(),
        "2cm"
    );
}

#[test]
fn spreadsheet_worksheet_round_trip() {
    let xml = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData><row r="1" spans="1:2"><c r="A1" t="s"><v>0</v></c><c r="B1"><f>SUM(1,2)</f><v>3</v></c></row></sheetData><drawing r:id="rId1"/></worksheet>"#;
    let ws = sml::elements::WORKSHEET.parse(xml).unwrap();
    let row = &ws.sheet_data.as_ref().unwrap().row[0];
    assert_eq!(row.r, Some(1));
    assert_eq!(row.c[0].t, Some(sml::ST_CellType::S));
    assert_eq!(row.c[0].v.as_deref(), Some("0"));
    assert_eq!(row.c[1].f.as_ref().unwrap().value, "SUM(1,2)");
    assert_eq!(ws.drawing.as_ref().unwrap().r_id.as_deref(), Some("rId1"));
    let out = sml::elements::WORKSHEET.to_xml(&ws);
    assert!(
        out.contains(r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#),
        "{out}"
    );
    assert!(out.contains(r#"<c r="B1"><f>SUM(1,2)</f><v>3</v></c>"#), "{out}");
    match validate_xml(&out) {
        Ok(_) => {}
        Err(e) => panic!("{e}\n{out}"),
    }
}

#[test]
fn presentation_slide_round_trip() {
    let xml = r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Title</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#;
    let slide = pml::elements::SLD.parse(xml).unwrap();
    let tree = &slide.c_sld.as_ref().unwrap().sp_tree.as_ref().unwrap();
    let pml::CT_GroupShape_Choice::Sp(sp) = &tree.choice[0] else {
        panic!("{:?}", tree.choice)
    };
    let para = &sp.tx_body.as_ref().unwrap().p[0];
    let dml::EG_TextRun::R(run) = &para.text_run[0] else {
        panic!()
    };
    assert_eq!(run.t.as_deref(), Some("Title"));
    let out = pml::elements::SLD.to_xml(&slide);
    match validate_xml(&out) {
        Ok(_) => {}
        Err(e) => panic!("{e}\n{out}"),
    }
    let mut back = pml::elements::SLD.parse(&out).unwrap();
    let mut expected = slide.clone();
    back.extra_attrs.retain(|a| !a.is_namespace_declaration());
    expected.extra_attrs.retain(|a| !a.is_namespace_declaration());
    assert_eq!(back, expected);
}

#[test]
fn every_global_element_parses_and_writes() {
    assert!(openxml_schema::GLOBAL_ELEMENTS.len() > 100);
    for &(ns, local) in openxml_schema::GLOBAL_ELEMENTS {
        let xml = format!(r#"<p:{local} xmlns:p="{}"/>"#, ns.uri());
        let out = openxml_schema::round_trip_xml(&xml)
            .unwrap_or_else(|| panic!("{local} not dispatched"))
            .unwrap_or_else(|e| panic!("{local}: {e}"));
        let (rns, rlocal) = openxml_xml::root_name(&out).unwrap();
        assert_eq!((rns, rlocal.as_str()), (ns, local));
        assert!(openxml_schema::is_known_root(ns, local));
    }
    assert!(!openxml_schema::is_known_root(Ns::W, "nonexistent"));
    assert!(openxml_schema::round_trip_xml("<unknown/>").is_none());
    assert!(openxml_schema::round_trip_xml("not xml").unwrap().is_err());
}

#[test]
fn wrong_root_is_rejected() {
    let err = wml::elements::DOCUMENT
        .parse(r#"<w:body xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#);
    assert!(matches!(err, Err(openxml_xml::Error::UnexpectedRoot { .. })));
}

#[test]
fn validation_reports_missing_required_content() {
    let xml = doc(r#"<w:p><w:pPr><w:pStyle/></w:pPr></w:p><w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>"#);
    let d = wml::elements::DOCUMENT.parse(&xml).unwrap();
    let issues = wml::elements::DOCUMENT.validate(&d);
    let text: Vec<String> = issues.iter().map(|i| i.to_string()).collect();
    assert!(
        text.contains(
            &"/w:document/w:body/w:p[1]/w:pPr/w:pStyle: missing required attribute w:val".to_owned()
        ),
        "{text:#?}"
    );
    assert!(text.contains(&"/w:document/w:body/w:tbl[2]: missing required child element w:tblPr".to_owned()));
    assert!(
        text.contains(&"/w:document/w:body/w:tbl[2]: missing required child element w:tblGrid".to_owned())
    );
    assert_eq!(issues.len(), 3, "{text:#?}");

    let ws = sml::elements::WORKSHEET
        .parse(r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#)
        .unwrap();
    let issues = sml::elements::WORKSHEET.validate(&ws);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].message, "missing required child element x:sheetData");
}

#[test]
fn documents_built_in_code_validate() {
    let d = wml::elements::DOCUMENT
        .parse(&doc(r#"<w:p><w:r><w:t>ok</w:t></w:r></w:p><w:sectPr/>"#))
        .unwrap();
    assert!(wml::elements::DOCUMENT.validate(&d).is_empty());
}
