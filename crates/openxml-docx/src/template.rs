//! Default parts of a new document and the catalog of built-in styles.
//!
//! The XML is written by hand for readability, parsed into the generated
//! schema types (which also puts every element in schema order) and
//! serialized from there.

use openxml_schema::shared_extended_properties as ep;
use openxml_schema::wml;
use openxml_xml::ElementDef;

const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

macro_rules! heading_xml {
    ($level:literal, $outline:literal, $size:literal, $color:literal, $before:literal) => {
        concat!(
            r#"<w:style w:type="paragraph" w:styleId="Heading"#,
            $level,
            r#""><w:name w:val="heading "#,
            $level,
            r#""/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=""#,
            $before,
            r#"" w:after="0"/><w:outlineLvl w:val=""#,
            $outline,
            r#""/></w:pPr><w:rPr><w:rFonts w:ascii="Calibri Light" w:hAnsi="Calibri Light" w:cs="Calibri Light"/><w:color w:val=""#,
            $color,
            r#""/><w:sz w:val=""#,
            $size,
            r#""/><w:szCs w:val=""#,
            $size,
            r#""/></w:rPr></w:style>"#
        )
    };
}

/// Built-in styles as `(style id, XML of the w:style element)`.
///
/// The names follow the names Word uses for its built-in styles so that the
/// documents behave like documents created by Word.
pub(crate) const BUILTIN_STYLES: &[(&str, &str)] = &[
    (
        "Normal",
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>"#,
    ),
    (
        "DefaultParagraphFont",
        r#"<w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont"><w:name w:val="Default Paragraph Font"/><w:uiPriority w:val="1"/><w:semiHidden/><w:unhideWhenUsed/></w:style>"#,
    ),
    (
        "TableNormal",
        r#"<w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>"#,
    ),
    (
        "NoList",
        r#"<w:style w:type="numbering" w:default="1" w:styleId="NoList"><w:name w:val="No List"/><w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/></w:style>"#,
    ),
    ("Heading1", heading_xml!(1, 0, "32", "2F5496", "240")),
    ("Heading2", heading_xml!(2, 1, "26", "2F5496", "40")),
    ("Heading3", heading_xml!(3, 2, "24", "1F3763", "40")),
    ("Heading4", heading_xml!(4, 3, "22", "2F5496", "40")),
    ("Heading5", heading_xml!(5, 4, "22", "2F5496", "40")),
    ("Heading6", heading_xml!(6, 5, "22", "1F3763", "40")),
    (
        "Title",
        r#"<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="10"/><w:qFormat/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/><w:contextualSpacing/></w:pPr><w:rPr><w:rFonts w:ascii="Calibri Light" w:hAnsi="Calibri Light" w:cs="Calibri Light"/><w:spacing w:val="-10"/><w:kern w:val="28"/><w:sz w:val="56"/><w:szCs w:val="56"/></w:rPr></w:style>"#,
    ),
    (
        "Subtitle",
        r#"<w:style w:type="paragraph" w:styleId="Subtitle"><w:name w:val="Subtitle"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="11"/><w:qFormat/><w:pPr><w:numPr><w:ilvl w:val="1"/></w:numPr></w:pPr><w:rPr><w:color w:val="5A5A5A"/><w:spacing w:val="15"/></w:rPr></w:style>"#,
    ),
    (
        "Quote",
        r#"<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="29"/><w:qFormat/><w:pPr><w:spacing w:before="200" w:after="160"/><w:ind w:left="864" w:right="864"/><w:jc w:val="center"/></w:pPr><w:rPr><w:i/><w:iCs/><w:color w:val="404040"/></w:rPr></w:style>"#,
    ),
    (
        "ListParagraph",
        r#"<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="34"/><w:qFormat/><w:pPr><w:ind w:left="720"/><w:contextualSpacing/></w:pPr></w:style>"#,
    ),
    (
        "Header",
        r#"<w:style w:type="paragraph" w:styleId="Header"><w:name w:val="header"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:pPr><w:tabs><w:tab w:val="center" w:pos="4680"/><w:tab w:val="right" w:pos="9360"/></w:tabs><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:style>"#,
    ),
    (
        "Footer",
        r#"<w:style w:type="paragraph" w:styleId="Footer"><w:name w:val="footer"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:pPr><w:tabs><w:tab w:val="center" w:pos="4680"/><w:tab w:val="right" w:pos="9360"/></w:tabs><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:style>"#,
    ),
    (
        "Hyperlink",
        r#"<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:basedOn w:val="DefaultParagraphFont"/><w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style>"#,
    ),
    (
        "TableGrid",
        r#"<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:basedOn w:val="TableNormal"/><w:uiPriority w:val="39"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders></w:tblPr></w:style>"#,
    ),
];

const DOC_DEFAULTS: &str = r#"<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:eastAsia="Calibri" w:hAnsi="Calibri" w:cs="Times New Roman"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US" w:eastAsia="en-US" w:bidi="ar-SA"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>"#;

/// Styles contained in the styles part of a new document.
const DEFAULT_STYLE_IDS: &[&str] = &[
    "Normal",
    "DefaultParagraphFont",
    "TableNormal",
    "NoList",
    "Heading1",
    "Heading2",
    "Heading3",
    "Heading4",
    "Heading5",
    "Heading6",
    "Title",
    "Subtitle",
    "Quote",
    "ListParagraph",
    "Header",
    "Footer",
    "Hyperlink",
    "TableGrid",
];

fn parse<T: openxml_xml::XmlRead>(def: &ElementDef<T>, xml: &str) -> T {
    def.parse(xml)
        .unwrap_or_else(|e| panic!("built-in template is invalid: {e}\n{xml}"))
}

/// The XML of a built-in style.
pub(crate) fn builtin_style_xml(style_id: &str) -> Option<&'static str> {
    BUILTIN_STYLES
        .iter()
        .find(|(id, _)| *id == style_id)
        .map(|(_, xml)| *xml)
}

/// Parses a built-in style.
pub(crate) fn builtin_style(style_id: &str) -> Option<wml::CT_Style> {
    let xml = builtin_style_xml(style_id)?;
    let wrapped = format!("<w:styles {W_NS}>{xml}</w:styles>");
    parse(&wml::elements::STYLES, &wrapped).style.into_iter().next()
}

/// The styles part of a new document.
pub(crate) fn default_styles() -> wml::CT_Styles {
    let mut xml = format!("<w:styles {W_NS}>{DOC_DEFAULTS}");
    for id in DEFAULT_STYLE_IDS {
        xml.push_str(builtin_style_xml(id).expect("style listed in the catalog"));
    }
    xml.push_str("</w:styles>");
    parse(&wml::elements::STYLES, &xml)
}

/// The settings part of a new document.
pub(crate) fn default_settings() -> wml::CT_Settings {
    let xml = format!(
        r#"<w:settings {W_NS}><w:zoom w:percent="100"/><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#
    );
    parse(&wml::elements::SETTINGS, &xml)
}

/// The font table of a new document.
pub(crate) fn default_font_table() -> wml::CT_FontsList {
    let xml = format!(
        r#"<w:fonts {W_NS}><w:font w:name="Calibri"><w:panose1 w:val="020F0502020204030204"/><w:charset w:val="00"/><w:family w:val="swiss"/><w:pitch w:val="variable"/></w:font><w:font w:name="Times New Roman"><w:panose1 w:val="02020603050405020304"/><w:charset w:val="00"/><w:family w:val="roman"/><w:pitch w:val="variable"/></w:font><w:font w:name="Calibri Light"><w:panose1 w:val="020F0302020204030204"/><w:charset w:val="00"/><w:family w:val="swiss"/><w:pitch w:val="variable"/></w:font></w:fonts>"#
    );
    parse(&wml::elements::FONTS, &xml)
}

/// The extended (application) properties of a new document.
pub(crate) fn default_app_properties() -> ep::CT_Properties {
    ep::CT_Properties {
        application: Some("openxml-rust".into()),
        doc_security: Some(0),
        scale_crop: Some(false),
        links_up_to_date: Some(false),
        shared_doc: Some(false),
        hyperlinks_changed: Some(false),
        ..Default::default()
    }
}

/// Header part content with a single paragraph in style `Header`/`Footer`.
pub(crate) fn header_footer(style_id: &str) -> wml::CT_HdrFtr {
    let p = wml::CT_P {
        p_pr: Some(Box::new(wml::CT_PPr {
            p_style: Some(crate::util::string_val(style_id)),
            ..Default::default()
        })),
        ..Default::default()
    };
    wml::CT_HdrFtr {
        block_level_elts: vec![wml::EG_BlockLevelElts::P(Box::new(p))],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_style_parses_with_its_id() {
        for (id, _) in BUILTIN_STYLES {
            let style = builtin_style(id).unwrap();
            assert_eq!(style.style_id.as_deref(), Some(*id));
            assert!(style.name.is_some(), "{id}");
        }
        assert!(builtin_style("Nope").is_none());
    }

    #[test]
    fn headings_have_outline_levels() {
        for level in 1..=6 {
            let style = builtin_style(&format!("Heading{level}")).unwrap();
            let outline = style.p_pr.as_ref().unwrap().outline_lvl.as_ref().unwrap().val;
            assert_eq!(outline, Some(level - 1));
            assert_eq!(style.name.unwrap().val.unwrap(), format!("heading {level}"));
        }
    }

    #[test]
    fn default_parts_are_complete() {
        let styles = default_styles();
        assert_eq!(styles.style.len(), DEFAULT_STYLE_IDS.len());
        assert!(styles.doc_defaults.is_some());
        assert!(default_settings().default_tab_stop.is_some());
        assert_eq!(default_font_table().font.len(), 3);
        assert_eq!(
            default_app_properties().application.as_deref(),
            Some("openxml-rust")
        );
        assert_eq!(header_footer("Header").block_level_elts.len(), 1);
    }
}
