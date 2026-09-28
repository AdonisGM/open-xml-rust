//! The parts of a new, empty presentation.
//!
//! The template follows the structure PowerPoint itself writes for a blank
//! widescreen (16:9) presentation: one slide master with six layouts, the
//! "Office" theme, presentation/view properties, table styles and document
//! properties. Every part validates against the ECMA-376 Transitional schemas.

use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{CoreProperties, Package, PartName, w3cdtf_now};

use openxml_core::Result;

/// Namespace declarations used by PresentationML parts.
const NS: &str = concat!(
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#
);

const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

/// Default slide width (16:9 widescreen), in EMU.
pub const SLIDE_WIDTH: i64 = 12_192_000;
/// Default slide height (16:9 widescreen), in EMU.
pub const SLIDE_HEIGHT: i64 = 6_858_000;
/// The identifier of the first slide master; layouts follow it.
pub const FIRST_MASTER_ID: u32 = 2_147_483_648;
/// Table style used by new tables ("Medium Style 2 - Accent 1").
pub const DEFAULT_TABLE_STYLE: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

/// A rectangle in EMU: `(x, y, cx, cy)`.
type Rect = (i64, i64, i64, i64);

fn xfrm(r: Rect) -> String {
    format!(
        r#"<a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#,
        r.0, r.1, r.2, r.3
    )
}

fn group_header() -> &'static str {
    concat!(
        r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>"#,
        r#"<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>"#,
        r#"<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#
    )
}

fn run(text: &str) -> String {
    format!(r#"<a:r><a:rPr lang="en-US"/><a:t>{text}</a:t></a:r>"#)
}

fn para(pr: &str, content: &str) -> String {
    format!(r#"<a:p>{pr}{content}<a:endParaRPr lang="en-US"/></a:p>"#)
}

fn date_field() -> String {
    format!(
        r#"<a:p><a:fld id="{{B5E1A7C0-6A48-4A2B-9E57-2F1C0D3A4B51}}" type="datetimeFigureOut"><a:rPr lang="en-US"/><a:t>1/1/2025</a:t></a:fld>{}</a:p>"#,
        r#"<a:endParaRPr lang="en-US"/>"#
    )
}

fn slide_number_field() -> String {
    format!(
        r#"<a:p><a:fld id="{{0C6D5B1E-4F2A-4D8B-A1E3-7B9C2D4E6F80}}" type="slidenum"><a:rPr lang="en-US"/><a:t>‹#›</a:t></a:fld>{}</a:p>"#,
        r#"<a:endParaRPr lang="en-US"/>"#
    )
}

/// A placeholder shape.
struct Ph<'a> {
    id: u32,
    name: &'a str,
    /// Attributes of `p:ph`, e.g. `type="title"`.
    ph: &'a str,
    rect: Option<Rect>,
    /// Extra shape properties after the transform (geometry, fill, line).
    sp_extra: &'a str,
    body_pr: &'a str,
    lst_style: &'a str,
    paragraphs: String,
}

impl Ph<'_> {
    fn xml(&self, locks: &str) -> String {
        let sp_pr = match self.rect {
            Some(r) => format!("<p:spPr>{}{}</p:spPr>", xfrm(r), self.sp_extra),
            None => format!("<p:spPr>{}</p:spPr>", self.sp_extra),
        };
        let lst = if self.lst_style.is_empty() {
            "<a:lstStyle/>".to_owned()
        } else {
            format!("<a:lstStyle>{}</a:lstStyle>", self.lst_style)
        };
        format!(
            concat!(
                r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name="{}"/><p:cNvSpPr>{}</p:cNvSpPr>"#,
                r#"<p:nvPr><p:ph {}/></p:nvPr></p:nvSpPr>{}<p:txBody>{}{}{}</p:txBody></p:sp>"#
            ),
            self.id, self.name, locks, self.ph, sp_pr, self.body_pr, lst, self.paragraphs
        )
    }
}

const LOCK_GROUP: &str = r#"<a:spLocks noGrp="1"/>"#;

fn master_levels_text() -> String {
    let mut s = para("", &run("Click to edit Master text styles"));
    for (lvl, label) in [
        (1, "Second level"),
        (2, "Third level"),
        (3, "Fourth level"),
        (4, "Fifth level"),
    ] {
        s.push_str(&para(&format!(r#"<a:pPr lvl="{lvl}"/>"#), &run(label)));
    }
    s
}

fn footer_placeholders(first_id: u32, dt_idx: u32) -> String {
    let dt = Ph {
        id: first_id,
        name: "Date Placeholder",
        ph: &format!(r#"type="dt" sz="half" idx="{dt_idx}""#),
        rect: None,
        sp_extra: "",
        body_pr: "<a:bodyPr/>",
        lst_style: "",
        paragraphs: date_field(),
    };
    let ftr = Ph {
        id: first_id + 1,
        name: "Footer Placeholder",
        ph: &format!(r#"type="ftr" sz="quarter" idx="{}""#, dt_idx + 1),
        rect: None,
        sp_extra: "",
        body_pr: "<a:bodyPr/>",
        lst_style: "",
        paragraphs: para("", ""),
    };
    let num = Ph {
        id: first_id + 2,
        name: "Slide Number Placeholder",
        ph: &format!(r#"type="sldNum" sz="quarter" idx="{}""#, dt_idx + 2),
        rect: None,
        sp_extra: "",
        body_pr: "<a:bodyPr/>",
        lst_style: "",
        paragraphs: slide_number_field(),
    };
    format!(
        "{}{}{}",
        dt.xml(LOCK_GROUP),
        ftr.xml(LOCK_GROUP),
        num.xml(LOCK_GROUP)
    )
}

fn scheme_fill(val: &str, mods: &str) -> String {
    format!(r#"<a:solidFill><a:schemeClr val="{val}">{mods}</a:schemeClr></a:solidFill>"#)
}

fn def_rpr(sz: u32, fill: &str, fonts: &str) -> String {
    format!(r#"<a:defRPr sz="{sz}" kern="1200">{fill}{fonts}</a:defRPr>"#)
}

const MINOR_FONTS: &str = r#"<a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/>"#;
const MAJOR_FONTS: &str = r#"<a:latin typeface="+mj-lt"/><a:ea typeface="+mj-ea"/><a:cs typeface="+mj-cs"/>"#;
const PPR_COMMON: &str = r#"defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1""#;

/// `a:lvlNpPr` elements of a plain text list style (used by `defaultTextStyle`).
fn plain_levels(sz: u32) -> String {
    let mut s = String::from(r#"<a:defPPr><a:defRPr lang="en-US"/></a:defPPr>"#);
    for lvl in 1..=9u32 {
        s.push_str(&format!(
            r#"<a:lvl{lvl}pPr marL="{}" algn="l" {PPR_COMMON}>{}</a:lvl{lvl}pPr>"#,
            (lvl - 1) * 457_200,
            def_rpr(sz, &scheme_fill("tx1", ""), MINOR_FONTS)
        ));
    }
    s
}

fn master_text_styles() -> String {
    let title = format!(
        concat!(
            r#"<p:titleStyle><a:lvl1pPr algn="l" {}><a:lnSpc><a:spcPct val="90000"/></a:lnSpc>"#,
            r#"<a:spcBef><a:spcPct val="0"/></a:spcBef><a:buNone/>{}</a:lvl1pPr></p:titleStyle>"#
        ),
        PPR_COMMON,
        def_rpr(4400, &scheme_fill("tx1", ""), MAJOR_FONTS)
    );
    let mut body = String::from("<p:bodyStyle>");
    for lvl in 1..=9u32 {
        let marl = 228_600 + (lvl - 1) * 457_200;
        let sz = match lvl {
            1 => 2800,
            2 => 2400,
            3 => 2000,
            _ => 1800,
        };
        let before = if lvl == 1 { 1000 } else { 500 };
        body.push_str(&format!(
            concat!(
                r#"<a:lvl{lvl}pPr marL="{marl}" indent="-228600" algn="l" {common}>"#,
                r#"<a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPts val="{before}"/></a:spcBef>"#,
                r#"<a:buFont typeface="Arial" panose="020B0604020202020204" pitchFamily="34" charset="0"/>"#,
                r#"<a:buChar char="•"/>{rpr}</a:lvl{lvl}pPr>"#
            ),
            lvl = lvl,
            marl = marl,
            common = PPR_COMMON,
            before = before,
            rpr = def_rpr(sz, &scheme_fill("tx1", ""), MINOR_FONTS)
        ));
    }
    body.push_str("</p:bodyStyle>");
    let other = format!("<p:otherStyle>{}</p:otherStyle>", plain_levels(1800));
    format!("<p:txStyles>{title}{body}{other}</p:txStyles>")
}

/// `ppt/presentation.xml`.
pub fn presentation_xml() -> String {
    format!(
        concat!(
            "{decl}<p:presentation {ns} saveSubsetFonts=\"1\">",
            r#"<p:sldMasterIdLst><p:sldMasterId id="{master}" r:id="rId1"/></p:sldMasterIdLst>"#,
            r#"<p:sldSz cx="{w}" cy="{h}"/><p:notesSz cx="6858000" cy="9144000"/>"#,
            "<p:defaultTextStyle>{levels}</p:defaultTextStyle></p:presentation>"
        ),
        decl = DECL,
        ns = NS,
        master = FIRST_MASTER_ID,
        w = SLIDE_WIDTH,
        h = SLIDE_HEIGHT,
        levels = plain_levels(1800)
    )
}

/// Layouts of the template: `(name, type, placeholders)`.
fn layouts() -> Vec<(&'static str, &'static str, String)> {
    let title_ph = |id: u32, name: &str| {
        Ph {
            id,
            name,
            ph: r#"type="title""#,
            rect: None,
            sp_extra: "",
            body_pr: "<a:bodyPr/>",
            lst_style: "",
            paragraphs: para("", &run("Click to edit Master title style")),
        }
        .xml(LOCK_GROUP)
    };
    let content_ph = |id: u32, name: &str, ph: &str, rect: Option<Rect>| {
        Ph {
            id,
            name,
            ph,
            rect,
            sp_extra: "",
            body_pr: "<a:bodyPr/>",
            lst_style: "",
            paragraphs: master_levels_text(),
        }
        .xml(LOCK_GROUP)
    };
    let title_slide = format!(
        "{}{}{}",
        Ph {
            id: 2,
            name: "Title 1",
            ph: r#"type="ctrTitle""#,
            rect: Some((1_524_000, 1_122_363, 9_144_000, 2_387_600)),
            sp_extra: "",
            body_pr: r#"<a:bodyPr anchor="b"/>"#,
            lst_style: r#"<a:lvl1pPr algn="ctr"><a:defRPr sz="6000"/></a:lvl1pPr>"#,
            paragraphs: para("", &run("Click to edit Master title style")),
        }
        .xml(LOCK_GROUP),
        Ph {
            id: 3,
            name: "Subtitle 2",
            ph: r#"type="subTitle" idx="1""#,
            rect: Some((1_524_000, 3_602_038, 9_144_000, 1_655_762)),
            sp_extra: "",
            body_pr: "<a:bodyPr/>",
            lst_style: r#"<a:lvl1pPr marL="0" indent="0" algn="ctr"><a:buNone/><a:defRPr sz="2400"/></a:lvl1pPr>"#,
            paragraphs: para("", &run("Click to edit Master subtitle style")),
        }
        .xml(LOCK_GROUP),
        footer_placeholders(4, 10)
    );
    let title_content = format!(
        "{}{}{}",
        title_ph(2, "Title 1"),
        content_ph(3, "Content Placeholder 2", r#"idx="1""#, None),
        footer_placeholders(4, 10)
    );
    let section = format!(
        "{}{}{}",
        Ph {
            id: 2,
            name: "Title 1",
            ph: r#"type="title""#,
            rect: Some((831_850, 1_709_738, 10_515_600, 2_852_737)),
            sp_extra: "",
            body_pr: r#"<a:bodyPr anchor="b"/>"#,
            lst_style: r#"<a:lvl1pPr><a:defRPr sz="6000"/></a:lvl1pPr>"#,
            paragraphs: para("", &run("Click to edit Master title style")),
        }
        .xml(LOCK_GROUP),
        Ph {
            id: 3,
            name: "Text Placeholder 2",
            ph: r#"type="body" idx="1""#,
            rect: Some((831_850, 4_589_463, 10_515_600, 1_500_187)),
            sp_extra: "",
            body_pr: "<a:bodyPr/>",
            lst_style: concat!(
                r#"<a:lvl1pPr marL="0" indent="0"><a:buNone/><a:defRPr sz="2400">"#,
                r#"<a:solidFill><a:schemeClr val="tx1"><a:tint val="75000"/></a:schemeClr></a:solidFill>"#,
                r#"</a:defRPr></a:lvl1pPr>"#
            ),
            paragraphs: para("", &run("Click to edit Master text styles")),
        }
        .xml(LOCK_GROUP),
        footer_placeholders(4, 10)
    );
    let two_content = format!(
        "{}{}{}{}",
        title_ph(2, "Title 1"),
        content_ph(
            3,
            "Content Placeholder 2",
            r#"sz="half" idx="1""#,
            Some((838_200, 1_825_625, 5_181_600, 4_351_338))
        ),
        content_ph(
            4,
            "Content Placeholder 3",
            r#"sz="half" idx="2""#,
            Some((6_172_200, 1_825_625, 5_181_600, 4_351_338))
        ),
        footer_placeholders(5, 10)
    );
    let title_only = format!("{}{}", title_ph(2, "Title 1"), footer_placeholders(3, 10));
    let blank = footer_placeholders(2, 10);
    vec![
        ("Title Slide", "title", title_slide),
        ("Title and Content", "obj", title_content),
        ("Section Header", "secHead", section),
        ("Two Content", "twoObj", two_content),
        ("Title Only", "titleOnly", title_only),
        ("Blank", "blank", blank),
    ]
}

fn layout_xml(name: &str, kind: &str, shapes: &str) -> String {
    format!(
        concat!(
            "{decl}<p:sldLayout {ns} type=\"{kind}\" preserve=\"1\"><p:cSld name=\"{name}\">",
            "<p:spTree>{header}{shapes}</p:spTree></p:cSld>",
            "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
        ),
        decl = DECL,
        ns = NS,
        kind = kind,
        name = name,
        header = group_header(),
        shapes = shapes
    )
}

/// `ppt/slideMasters/slideMaster1.xml`; layouts are related as `rId1..rIdN`.
fn master_xml(layout_count: usize) -> String {
    let title = Ph {
        id: 2,
        name: "Title Placeholder 1",
        ph: r#"type="title""#,
        rect: Some((838_200, 365_125, 10_515_600, 1_325_563)),
        sp_extra: r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#,
        body_pr: r#"<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="ctr"><a:normAutofit/></a:bodyPr>"#,
        lst_style: "",
        paragraphs: para("", &run("Click to edit Master title style")),
    };
    let body = Ph {
        id: 3,
        name: "Text Placeholder 2",
        ph: r#"type="body" idx="1""#,
        rect: Some((838_200, 1_825_625, 10_515_600, 4_351_338)),
        sp_extra: r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#,
        body_pr: r#"<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0"><a:normAutofit/></a:bodyPr>"#,
        lst_style: "",
        paragraphs: master_levels_text(),
    };
    let footer_style = |algn: &str| {
        format!(
            r#"<a:lvl1pPr algn="{algn}">{}</a:lvl1pPr>"#,
            def_rpr(1200, &scheme_fill("tx1", r#"<a:tint val="75000"/>"#), "")
        )
    };
    let small = r#"<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="ctr"/>"#;
    let geom = r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#;
    let dt = Ph {
        id: 4,
        name: "Date Placeholder 3",
        ph: r#"type="dt" sz="half" idx="2""#,
        rect: Some((838_200, 6_356_350, 2_743_200, 365_125)),
        sp_extra: geom,
        body_pr: small,
        lst_style: &footer_style("l"),
        paragraphs: date_field(),
    };
    let ftr = Ph {
        id: 5,
        name: "Footer Placeholder 4",
        ph: r#"type="ftr" sz="quarter" idx="3""#,
        rect: Some((4_038_600, 6_356_350, 4_114_800, 365_125)),
        sp_extra: geom,
        body_pr: small,
        lst_style: &footer_style("ctr"),
        paragraphs: para("", ""),
    };
    let num = Ph {
        id: 6,
        name: "Slide Number Placeholder 5",
        ph: r#"type="sldNum" sz="quarter" idx="4""#,
        rect: Some((8_610_600, 6_356_350, 2_743_200, 365_125)),
        sp_extra: geom,
        body_pr: small,
        lst_style: &footer_style("r"),
        paragraphs: slide_number_field(),
    };
    let mut ids = String::new();
    for i in 0..layout_count {
        ids.push_str(&format!(
            r#"<p:sldLayoutId id="{}" r:id="rId{}"/>"#,
            FIRST_MASTER_ID as usize + 1 + i,
            i + 1
        ));
    }
    format!(
        concat!(
            "{decl}<p:sldMaster {ns}><p:cSld>",
            r#"<p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg>"#,
            "<p:spTree>{header}{shapes}</p:spTree></p:cSld>{clr_map}",
            "<p:sldLayoutIdLst>{ids}</p:sldLayoutIdLst>{styles}</p:sldMaster>"
        ),
        decl = DECL,
        ns = NS,
        header = group_header(),
        shapes = [title, body, dt, ftr, num]
            .iter()
            .map(|p| p.xml(LOCK_GROUP))
            .collect::<String>(),
        clr_map = CLR_MAP,
        ids = ids,
        styles = master_text_styles()
    )
}

/// The default colour mapping of masters.
pub const CLR_MAP: &str = concat!(
    r#"<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" "#,
    r#"accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#
);

fn gradient(stops: &[(u32, &str)]) -> String {
    let gs: String = stops
        .iter()
        .map(|(pos, mods)| {
            format!(r#"<a:gs pos="{pos}"><a:schemeClr val="phClr">{mods}</a:schemeClr></a:gs>"#)
        })
        .collect();
    format!(
        r#"<a:gradFill rotWithShape="1"><a:gsLst>{gs}</a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>"#
    )
}

/// A theme part (`ppt/theme/themeN.xml`) with the Office colour and font schemes.
pub fn theme_xml(name: &str) -> String {
    let colors = [
        ("accent1", "4472C4"),
        ("accent2", "ED7D31"),
        ("accent3", "A5A5A5"),
        ("accent4", "FFC000"),
        ("accent5", "5B9BD5"),
        ("accent6", "70AD47"),
        ("hlink", "0563C1"),
        ("folHlink", "954F72"),
    ];
    let mut clr = String::from(concat!(
        r#"<a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>"#,
        r#"<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>"#,
        r#"<a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>"#
    ));
    for (slot, hex) in colors {
        clr.push_str(&format!(r#"<a:{slot}><a:srgbClr val="{hex}"/></a:{slot}>"#));
    }
    clr.push_str("</a:clrScheme>");
    let fonts = concat!(
        r#"<a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light" panose="020F0302020204030204"/>"#,
        r#"<a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont>"#,
        r#"<a:latin typeface="Calibri" panose="020F0502020204030204"/><a:ea typeface=""/><a:cs typeface=""/>"#,
        r#"</a:minorFont></a:fontScheme>"#
    );
    let fills = format!(
        "<a:fillStyleLst>{}{}{}</a:fillStyleLst>",
        scheme_fill("phClr", ""),
        gradient(&[
            (
                0,
                r#"<a:lumMod val="110000"/><a:satMod val="105000"/><a:tint val="67000"/>"#
            ),
            (
                50_000,
                r#"<a:lumMod val="105000"/><a:satMod val="103000"/><a:tint val="73000"/>"#
            ),
            (
                100_000,
                r#"<a:lumMod val="105000"/><a:satMod val="109000"/><a:tint val="81000"/>"#
            ),
        ]),
        gradient(&[
            (
                0,
                r#"<a:satMod val="103000"/><a:lumMod val="102000"/><a:tint val="94000"/>"#
            ),
            (
                50_000,
                r#"<a:satMod val="110000"/><a:lumMod val="100000"/><a:shade val="100000"/>"#
            ),
            (
                100_000,
                r#"<a:lumMod val="99000"/><a:satMod val="120000"/><a:shade val="78000"/>"#
            ),
        ])
    );
    let line = |w: u32| {
        format!(
            r#"<a:ln w="{w}" cap="flat" cmpd="sng" algn="ctr">{}<a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>"#,
            scheme_fill("phClr", "")
        )
    };
    let lines = format!(
        "<a:lnStyleLst>{}{}{}</a:lnStyleLst>",
        line(6350),
        line(12_700),
        line(19_050)
    );
    let effects = concat!(
        r#"<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle>"#,
        r#"<a:effectStyle><a:effectLst><a:outerShdw blurRad="57150" dist="19050" dir="5400000" algn="ctr" rotWithShape="0">"#,
        r#"<a:srgbClr val="000000"><a:alpha val="63000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle>"#,
        r#"</a:effectStyleLst>"#
    );
    let backgrounds = format!(
        "<a:bgFillStyleLst>{}{}{}</a:bgFillStyleLst>",
        scheme_fill("phClr", ""),
        scheme_fill("phClr", r#"<a:tint val="95000"/><a:satMod val="170000"/>"#),
        gradient(&[
            (
                0,
                r#"<a:tint val="93000"/><a:satMod val="150000"/><a:shade val="98000"/><a:lumMod val="102000"/>"#
            ),
            (
                50_000,
                r#"<a:tint val="98000"/><a:satMod val="130000"/><a:shade val="90000"/><a:lumMod val="103000"/>"#
            ),
            (100_000, r#"<a:shade val="63000"/><a:satMod val="120000"/>"#),
        ])
    );
    format!(
        concat!(
            "{decl}<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"{name}\">",
            "<a:themeElements>{clr}{fonts}<a:fmtScheme name=\"Office\">{fills}{lines}{effects}{backgrounds}</a:fmtScheme>",
            "</a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"
        ),
        decl = DECL,
        name = name,
        clr = clr,
        fonts = fonts,
        fills = fills,
        lines = lines,
        effects = effects,
        backgrounds = backgrounds
    )
}

fn pres_props_xml() -> String {
    format!("{DECL}<p:presentationPr {NS}/>")
}

fn view_props_xml() -> String {
    format!(
        concat!(
            "{}<p:viewPr {}>",
            r#"<p:normalViewPr><p:restoredLeft sz="15620"/><p:restoredTop sz="94660"/></p:normalViewPr>"#,
            r#"<p:slideViewPr><p:cSldViewPr snapToGrid="0"><p:cViewPr varScale="1"><p:scale>"#,
            r#"<a:sx n="100" d="100"/><a:sy n="100" d="100"/></p:scale><p:origin x="0" y="0"/></p:cViewPr>"#,
            r#"<p:guideLst/></p:cSldViewPr></p:slideViewPr><p:gridSpacing cx="76200" cy="76200"/></p:viewPr>"#
        ),
        DECL, NS
    )
}

fn table_styles_xml() -> String {
    format!(
        r#"{DECL}<a:tblStyleLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" def="{DEFAULT_TABLE_STYLE}"/>"#
    )
}

fn app_xml() -> String {
    format!(
        concat!(
            "{}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" ",
            "xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">",
            "<TotalTime>0</TotalTime><Words>0</Words><Application>openxml-rust</Application>",
            "<PresentationFormat>Widescreen</PresentationFormat><Paragraphs>0</Paragraphs>",
            "<Slides>0</Slides><Notes>0</Notes><HiddenSlides>0</HiddenSlides><MMClips>0</MMClips>",
            "<ScaleCrop>false</ScaleCrop><LinksUpToDate>false</LinksUpToDate><SharedDoc>false</SharedDoc>",
            "<HyperlinksChanged>false</HyperlinksChanged><AppVersion>16.0000</AppVersion></Properties>"
        ),
        DECL
    )
}

/// `ppt/notesMasters/notesMaster1.xml`; its theme is related as `rId1`.
pub fn notes_master_xml() -> String {
    let img = Ph {
        id: 2,
        name: "Slide Image Placeholder 1",
        ph: r#"type="sldImg" idx="2""#,
        rect: Some((685_800, 1_143_000, 5_486_400, 3_086_100)),
        sp_extra: concat!(
            r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/>"#,
            r#"<a:ln w="12700"><a:solidFill><a:prstClr val="black"/></a:solidFill></a:ln>"#
        ),
        body_pr: r#"<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="ctr"/>"#,
        lst_style: "",
        paragraphs: para("", ""),
    };
    let body = Ph {
        id: 3,
        name: "Notes Placeholder 2",
        ph: r#"type="body" sz="quarter" idx="3""#,
        rect: Some((685_800, 4_400_550, 5_486_400, 3_600_450)),
        sp_extra: r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#,
        body_pr: r#"<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0"/>"#,
        lst_style: "",
        paragraphs: master_levels_text(),
    };
    let num = Ph {
        id: 4,
        name: "Slide Number Placeholder 3",
        ph: r#"type="sldNum" sz="quarter" idx="5""#,
        rect: Some((3_884_613, 8_685_213, 2_971_800, 458_787)),
        sp_extra: r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#,
        body_pr: r#"<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="b"/>"#,
        lst_style: &format!(r#"<a:lvl1pPr algn="r">{}</a:lvl1pPr>"#, def_rpr(1200, "", "")),
        paragraphs: slide_number_field(),
    };
    let lock_img = r#"<a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/>"#;
    let mut style = String::new();
    for lvl in 1..=9u32 {
        style.push_str(&format!(
            r#"<a:lvl{lvl}pPr marL="{}" algn="l" {PPR_COMMON}>{}</a:lvl{lvl}pPr>"#,
            (lvl - 1) * 457_200,
            def_rpr(1200, &scheme_fill("tx1", ""), MINOR_FONTS)
        ));
    }
    format!(
        concat!(
            "{decl}<p:notesMaster {ns}><p:cSld>",
            r#"<p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg>"#,
            "<p:spTree>{header}{img}{body}{num}</p:spTree></p:cSld>{clr_map}",
            "<p:notesStyle>{style}</p:notesStyle></p:notesMaster>"
        ),
        decl = DECL,
        ns = NS,
        header = group_header(),
        img = img.xml(lock_img),
        body = body.xml(LOCK_GROUP),
        num = num.xml(LOCK_GROUP),
        clr_map = CLR_MAP,
        style = style
    )
}

/// A notes slide whose body placeholder contains `paragraphs` (DrawingML `a:p` XML).
pub fn notes_slide_xml(paragraphs: &str) -> String {
    format!(
        concat!(
            "{decl}<p:notes {ns}><p:cSld><p:spTree>{header}",
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/><p:cNvSpPr>"#,
            r#"<a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg"/></p:nvPr>"#,
            r#"</p:nvSpPr><p:spPr/></p:sp>"#,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>"#,
            r#"<p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/>"#,
            "<p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp>",
            "</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"
        ),
        decl = DECL,
        ns = NS,
        header = group_header(),
        paragraphs = paragraphs
    )
}

/// An empty slide (only the shape tree root).
pub fn empty_slide_xml() -> String {
    format!(
        concat!(
            "{decl}<p:sld {ns}><p:cSld><p:spTree>{header}</p:spTree></p:cSld>",
            "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
        ),
        decl = DECL,
        ns = NS,
        header = group_header()
    )
}

fn pn(s: &str) -> PartName {
    PartName::new(s).expect("template part names are valid")
}

/// Builds the package of a new, empty presentation.
pub fn blank_package() -> Result<Package> {
    let mut pkg = Package::new();
    let pres = pn("/ppt/presentation.xml");
    let master = pn("/ppt/slideMasters/slideMaster1.xml");
    let theme = pn("/ppt/theme/theme1.xml");

    pkg.add_part(
        pres.clone(),
        ct::PML_PRESENTATION,
        presentation_xml().into_bytes(),
    )?;
    pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &pres)?;

    let layouts = layouts();
    pkg.add_part(
        master.clone(),
        ct::PML_SLIDE_MASTER,
        master_xml(layouts.len()).into_bytes(),
    )?;
    // presentation.xml refers to the master as rId1.
    pkg.add_relationship(Some(&pres), rel_types::SLIDE_MASTER, &master)?;
    for (i, (name, kind, shapes)) in layouts.iter().enumerate() {
        let part = pn(&format!("/ppt/slideLayouts/slideLayout{}.xml", i + 1));
        pkg.add_part(
            part.clone(),
            ct::PML_SLIDE_LAYOUT,
            layout_xml(name, kind, shapes).into_bytes(),
        )?;
        pkg.add_relationship(Some(&part), rel_types::SLIDE_MASTER, &master)?;
        // The master refers to its layouts as rId1..rIdN, in order.
        pkg.add_relationship(Some(&master), rel_types::SLIDE_LAYOUT, &part)?;
    }
    pkg.add_part(theme.clone(), ct::THEME, theme_xml("Office Theme").into_bytes())?;
    pkg.add_relationship(Some(&master), rel_types::THEME, &theme)?;
    pkg.add_relationship(Some(&pres), rel_types::THEME, &theme)?;

    for (path, content_type, rel, xml) in [
        (
            "/ppt/presProps.xml",
            ct::PML_PRES_PROPS,
            rel_types::PRES_PROPS,
            pres_props_xml(),
        ),
        (
            "/ppt/viewProps.xml",
            ct::PML_VIEW_PROPS,
            rel_types::VIEW_PROPS,
            view_props_xml(),
        ),
        (
            "/ppt/tableStyles.xml",
            ct::PML_TABLE_STYLES,
            rel_types::TABLE_STYLES,
            table_styles_xml(),
        ),
    ] {
        let part = pn(path);
        pkg.add_part(part.clone(), content_type, xml.into_bytes())?;
        pkg.add_relationship(Some(&pres), rel, &part)?;
    }

    let app = pn("/docProps/app.xml");
    pkg.add_part(app.clone(), ct::EXTENDED_PROPERTIES, app_xml().into_bytes())?;
    pkg.add_relationship(None, rel_types::EXTENDED_PROPERTIES, &app)?;
    let now = w3cdtf_now();
    pkg.set_core_properties(&CoreProperties {
        title: Some("Presentation".into()),
        creator: Some("openxml-rust".into()),
        last_modified_by: Some("openxml-rust".into()),
        revision: Some("1".into()),
        created: Some(now.clone()),
        modified: Some(now),
        ..Default::default()
    })?;
    Ok(pkg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openxml_xml::RawElement;

    #[test]
    fn every_template_part_is_well_formed() {
        let mut parts = vec![
            presentation_xml(),
            master_xml(6),
            theme_xml("T"),
            pres_props_xml(),
            view_props_xml(),
            table_styles_xml(),
            app_xml(),
            notes_master_xml(),
            notes_slide_xml("<a:p/>"),
            empty_slide_xml(),
        ];
        for (name, kind, shapes) in layouts() {
            parts.push(layout_xml(name, kind, &shapes));
        }
        for xml in parts {
            RawElement::parse(&xml).unwrap_or_else(|e| panic!("{e}\n{xml}"));
        }
    }

    #[test]
    fn master_lists_one_id_per_layout() {
        let xml = master_xml(6);
        assert_eq!(xml.matches("<p:sldLayoutId ").count(), 6);
        assert!(xml.contains(r#"<p:sldLayoutId id="2147483649" r:id="rId1"/>"#));
        assert!(xml.contains(r#"<p:sldLayoutId id="2147483654" r:id="rId6"/>"#));
    }

    #[test]
    fn layout_placeholder_ids_are_unique_per_layout() {
        for (name, _, shapes) in layouts() {
            let root = RawElement::parse(&format!("<r {NS}>{shapes}</r>")).unwrap();
            let mut ids: Vec<&str> = root
                .descendants()
                .into_iter()
                .filter(|e| &*e.name.local == "cNvPr")
                .filter_map(|e| e.attr(openxml_xml::Ns::NONE, "id"))
                .collect();
            let n = ids.len();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), n, "{name}");
        }
    }

    #[test]
    fn blank_package_has_the_expected_parts() {
        let pkg = blank_package().unwrap();
        let names: Vec<String> = pkg.parts().map(|(n, _)| n.to_string()).collect();
        for expected in [
            "/ppt/presentation.xml",
            "/ppt/slideMasters/slideMaster1.xml",
            "/ppt/slideLayouts/slideLayout1.xml",
            "/ppt/slideLayouts/slideLayout6.xml",
            "/ppt/theme/theme1.xml",
            "/ppt/presProps.xml",
            "/ppt/viewProps.xml",
            "/ppt/tableStyles.xml",
            "/docProps/app.xml",
            "/docProps/core.xml",
        ] {
            assert!(names.iter().any(|n| n == expected), "missing {expected}");
        }
        let pres = pn("/ppt/presentation.xml");
        let rels = pkg.relationships(Some(&pres)).unwrap();
        assert_eq!(rels.get("rId1").unwrap().rel_type, rel_types::SLIDE_MASTER);
        let master = pn("/ppt/slideMasters/slideMaster1.xml");
        let mrels = pkg.relationships(Some(&master)).unwrap();
        for i in 1..=6 {
            assert_eq!(
                mrels.get(&format!("rId{i}")).unwrap().rel_type,
                rel_types::SLIDE_LAYOUT
            );
        }
    }
}
