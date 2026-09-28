//! Text boxes, simple shapes and text watermarks as VML (`w:pict`), the
//! drawing format of ECMA-376 WordprocessingML that every consumer reads.
//! (Word's `wps` shapes are a Microsoft extension and are not produced.)

use openxml_core::{Error, Length, Result};
use openxml_opc::PartName;
use openxml_schema::wml::{self, EG_BlockLevelElts, EG_PContent, EG_RunInnerContent};
use openxml_xml::{Ns, RawElement, RawNode, escape_attr};

use crate::document::{Document, Shared};
use crate::drawing::Wrap;
use crate::paragraph::ParagraphMut;
use crate::section::{HeaderFooterKind, HeaderFooterType};
use crate::text;
use crate::util::parse_rgb;

const NAMESPACES: &str = r#"xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:w10="urn:schemas-microsoft-com:office:word" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

/// Id prefix of the watermark shapes (the name Word uses).
const WATERMARK_ID: &str = "PowerPlusWaterMarkObject";

/// Geometry of a simple shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    /// Rectangle.
    Rectangle,
    /// Rectangle with rounded corners.
    RoundedRectangle,
    /// Ellipse.
    Ellipse,
    /// Straight line from the top-left to the bottom-right corner of the box.
    Line,
}

/// Size, colors and placement of a shape or text box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeOptions {
    /// Width.
    pub width: Length,
    /// Height.
    pub height: Length,
    /// Fill color (`RRGGBB`); not filled when `None`.
    pub fill: Option<String>,
    /// Outline color (`RRGGBB`); no outline when `None`.
    pub stroke: Option<String>,
    /// Outline width.
    pub stroke_weight: Length,
    /// Floating position: offsets from the column (horizontal) and the
    /// paragraph (vertical); inline when `None`.
    pub position: Option<(Length, Length)>,
    /// Text wrapping of a floating shape.
    pub wrap: Wrap,
    /// URL opened when the shape is clicked.
    pub hyperlink: Option<String>,
}

impl ShapeOptions {
    /// An inline shape with a thin black outline and no fill.
    pub fn new(width: Length, height: Length) -> Self {
        ShapeOptions {
            width,
            height,
            fill: None,
            stroke: Some("000000".into()),
            stroke_weight: Length::pt(0.75),
            position: None,
            wrap: Wrap::Square,
            hyperlink: None,
        }
    }
}

/// Formats a length in points for VML styles.
fn pt(len: Length) -> String {
    let v = (len.as_pt() * 100.0).round() / 100.0;
    format!("{v}pt")
}

fn attr(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape_attr(out, value);
    out.push('"');
}

fn color(hex: &str) -> Result<String> {
    let rgb = parse_rgb(hex)?;
    Ok(format!("#{:02X}{:02X}{:02X}", rgb.0[0], rgb.0[1], rgb.0[2]))
}

/// Opening tag attributes shared by every shape.
fn shape_attrs(options: &ShapeOptions, id: u32, is_line: bool) -> Result<String> {
    let mut style = String::new();
    if let Some((x, y)) = options.position {
        let z = if options.wrap == Wrap::BehindText {
            -251_658_240 + i64::from(id)
        } else {
            251_658_240 + i64::from(id)
        };
        style.push_str(&format!(
            "position:absolute;margin-left:{};margin-top:{};",
            pt(x),
            pt(y)
        ));
        style.push_str(&format!(
            "width:{};height:{};z-index:{z};mso-position-horizontal-relative:text;mso-position-vertical-relative:text",
            pt(options.width),
            pt(options.height)
        ));
    } else {
        style.push_str(&format!(
            "width:{};height:{}",
            pt(options.width),
            pt(options.height)
        ));
    }
    let mut out = String::new();
    attr(&mut out, "id", &format!("_x0000_s{}", 1024 + id));
    attr(&mut out, "style", &style);
    if let Some(url) = &options.hyperlink {
        attr(&mut out, "href", url);
    }
    if is_line {
        attr(&mut out, "from", "0,0");
        attr(
            &mut out,
            "to",
            &format!("{},{}", pt(options.width), pt(options.height)),
        );
    } else {
        match &options.fill {
            Some(fill) => attr(&mut out, "fillcolor", &color(fill)?),
            None => attr(&mut out, "filled", "f"),
        }
    }
    match &options.stroke {
        Some(stroke) => {
            attr(&mut out, "strokecolor", &color(stroke)?);
            attr(&mut out, "strokeweight", &pt(options.stroke_weight));
        }
        None => attr(&mut out, "stroked", "f"),
    }
    Ok(out)
}

fn wrap_element(options: &ShapeOptions) -> &'static str {
    match (options.position, options.wrap) {
        (None, _) | (_, Wrap::BehindText | Wrap::InFrontOfText) => "",
        (_, Wrap::Square) => r#"<w10:wrap type="square"/>"#,
        (_, Wrap::Tight) => r#"<w10:wrap type="tight"/>"#,
        (_, Wrap::TopAndBottom) => r#"<w10:wrap type="topAndBottom"/>"#,
    }
}

/// Builds the VML element of a shape (without text box).
fn shape_element(kind: ShapeKind, options: &ShapeOptions, id: u32) -> Result<RawElement> {
    if options.width.as_emu() <= 0 || options.height.as_emu() < 0 {
        return Err(Error::InvalidArgument("shape size must be positive".into()));
    }
    let (tag, extra) = match kind {
        ShapeKind::Rectangle => ("v:rect", String::new()),
        ShapeKind::RoundedRectangle => ("v:roundrect", r#" arcsize="10923f""#.to_owned()),
        ShapeKind::Ellipse => ("v:oval", String::new()),
        ShapeKind::Line => ("v:line", String::new()),
    };
    let attrs = shape_attrs(options, id, kind == ShapeKind::Line)?;
    let xml = format!(
        "<{tag} {NAMESPACES}{attrs}{extra}>{}</{tag}>",
        wrap_element(options)
    );
    RawElement::parse(&xml).map_err(|source| Error::Xml {
        part: "VML shape".into(),
        source,
    })
}

fn pict(shapes: Vec<RawElement>) -> EG_RunInnerContent {
    EG_RunInnerContent::Pict(Box::new(wml::CT_Picture {
        any: shapes,
        ..Default::default()
    }))
}

/// Paragraphs being written into a text box.
#[derive(Debug)]
pub struct TextBoxContent<'a> {
    blocks: &'a mut Vec<EG_BlockLevelElts>,
    shared: &'a mut Shared,
    part: PartName,
}

impl TextBoxContent<'_> {
    /// Appends a paragraph with `text` and returns it for formatting.
    pub fn add_paragraph(&mut self, text: &str) -> ParagraphMut<'_> {
        self.blocks.push(EG_BlockLevelElts::P(Box::default()));
        let Some(EG_BlockLevelElts::P(p)) = self.blocks.last_mut() else {
            unreachable!("a paragraph was just added")
        };
        let mut handle = ParagraphMut::new(p, self.shared, self.part.clone());
        if !text.is_empty() {
            handle.add_text(text);
        }
        handle
    }
}

impl ParagraphMut<'_> {
    /// Appends a simple VML shape.
    pub fn add_shape(&mut self, kind: ShapeKind, options: &ShapeOptions) -> Result<&mut Self> {
        self.shared.next_drawing_id += 1;
        let shape = shape_element(kind, options, self.shared.next_drawing_id)?;
        self.p.p_content.push(EG_PContent::R(Box::new(wml::CT_R {
            run_inner_content: vec![pict(vec![shape])],
            ..Default::default()
        })));
        Ok(self)
    }

    /// Appends a text box (a VML rectangle with `v:textbox`) whose content
    /// is written by `build` with ordinary paragraphs.
    ///
    /// ```
    /// use openxml_docx::{Document, Length, ShapeOptions};
    ///
    /// let mut doc = Document::new();
    /// let mut options = ShapeOptions::new(Length::inches(2.0), Length::inches(1.0));
    /// options.fill = Some("FFF2CC".into());
    /// doc.add_paragraph("").add_text_box(&options, |content| {
    ///     content.add_paragraph("Note").add_run("!").bold(true);
    ///     Ok(())
    /// })?;
    /// assert_eq!(doc.text_boxes(), ["Note!"]);
    /// # Ok::<(), openxml_docx::Error>(())
    /// ```
    pub fn add_text_box(
        &mut self,
        options: &ShapeOptions,
        build: impl FnOnce(&mut TextBoxContent<'_>) -> Result<()>,
    ) -> Result<&mut Self> {
        let mut content = wml::CT_TxbxContent::default();
        build(&mut TextBoxContent {
            blocks: &mut content.block_level_elts,
            shared: self.shared,
            part: self.part.clone(),
        })?;
        if content.block_level_elts.is_empty() {
            content
                .block_level_elts
                .push(EG_BlockLevelElts::P(Box::default()));
        }
        self.shared.next_drawing_id += 1;
        let mut shape = shape_element(ShapeKind::Rectangle, options, self.shared.next_drawing_id)?;
        let mut textbox = RawElement::new(Ns::V, "textbox");
        textbox.children.push(RawNode::Element(RawElement::from_typed(
            &content,
            Ns::W,
            "txbxContent",
        )));
        // The text box comes before the wrapping element.
        shape.children.insert(0, RawNode::Element(textbox));
        self.p.p_content.push(EG_PContent::R(Box::new(wml::CT_R {
            run_inner_content: vec![pict(vec![shape])],
            ..Default::default()
        })));
        Ok(self)
    }
}

/// Text box contents (`w:txbxContent`) inside a raw VML or DrawingML tree.
fn text_box_contents(e: &RawElement, out: &mut Vec<wml::CT_TxbxContent>) {
    for d in e.descendants() {
        if d.name.is(Ns::W, "txbxContent")
            && let Ok(c) = d.to_typed::<wml::CT_TxbxContent>()
        {
            out.push(c);
        }
    }
}

/// The watermark shape XML (`shapetype` 136 is WordArt plain text).
fn watermark_shapes(text: &str, id: u32) -> Result<Vec<RawElement>> {
    let mut string = String::new();
    escape_attr(&mut string, text);
    let shapetype = format!(
        r##"<v:shapetype {NAMESPACES} id="_x0000_t136" coordsize="21600,21600" o:spt="136" adj="10800" path="m@7,l@8,m@5,21600l@6,21600e"><v:formulas><v:f eqn="sum #0 0 10800"/><v:f eqn="prod #0 2 1"/><v:f eqn="sum 21600 0 @1"/><v:f eqn="sum 0 0 @2"/><v:f eqn="sum 21600 0 @3"/><v:f eqn="if @0 @3 0"/><v:f eqn="if @0 21600 @1"/><v:f eqn="if @0 0 @2"/><v:f eqn="if @0 @4 21600"/><v:f eqn="mid @5 @6"/><v:f eqn="mid @8 @5"/><v:f eqn="mid @7 @8"/><v:f eqn="mid @6 @7"/><v:f eqn="sum @6 0 @5"/></v:formulas><v:path textpathok="t" o:connecttype="custom" o:connectlocs="@9,0;@10,10800;@11,21600;@12,10800" o:connectangles="270,180,90,0"/><v:textpath on="t" fitshape="t"/><v:handles><v:h position="#0,bottomRight" xrange="6629,14971"/></v:handles><o:lock v:ext="edit" text="t" shapetype="t"/></v:shapetype>"##
    );
    let shape = format!(
        r##"<v:shape {NAMESPACES} id="{WATERMARK_ID}{id}" o:spid="_x0000_s{}" type="#_x0000_t136" style="position:absolute;margin-left:0;margin-top:0;width:468pt;height:117pt;rotation:315;z-index:-251655168;mso-position-horizontal:center;mso-position-horizontal-relative:margin;mso-position-vertical:center;mso-position-vertical-relative:margin" o:allowincell="f" fillcolor="silver" stroked="f"><v:fill opacity=".5"/><v:textpath style="font-family:&quot;Calibri&quot;;font-size:1pt" string="{string}"/><w10:wrap anchorx="margin" anchory="margin"/></v:shape>"##,
        2048 + id
    );
    [shapetype, shape]
        .iter()
        .map(|xml| {
            RawElement::parse(xml).map_err(|source| Error::Xml {
                part: "VML watermark".into(),
                source,
            })
        })
        .collect()
}

fn is_watermark(pict: &wml::CT_Picture) -> bool {
    pict.any.iter().any(|e| {
        e.name.is(Ns::V, "shape")
            && e.attr(Ns::NONE, "id")
                .is_some_and(|id| id.starts_with(WATERMARK_ID))
    })
}

fn watermark_text(pict: &wml::CT_Picture) -> Option<String> {
    pict.any
        .iter()
        .filter(|e| {
            e.name.is(Ns::V, "shape")
                && e.attr(Ns::NONE, "id")
                    .is_some_and(|id| id.starts_with(WATERMARK_ID))
        })
        .find_map(|e| {
            e.child(Ns::V, "textpath")?
                .attr(Ns::NONE, "string")
                .map(str::to_owned)
        })
}

/// Removes watermark pictures from a header; returns whether any was found.
fn strip_watermarks(blocks: &mut Vec<EG_BlockLevelElts>) -> bool {
    crate::markup::retain_in_blocks(blocks, &|_| false, &|_| false, &mut |r| {
        let before = r.run_inner_content.len();
        r.run_inner_content
            .retain(|c| !matches!(c, EG_RunInnerContent::Pict(p) if is_watermark(p)));
        r.run_inner_content.len() != before
    })
}

impl Document {
    /// Text of each text box in the body (VML `v:textbox` and DrawingML
    /// `wps:txbx`), in document order.
    pub fn text_boxes(&self) -> Vec<String> {
        let mut contents = Vec::new();
        crate::walk::walk_blocks_ref(&self.body().block_level_elts, &mut |p| {
            for r in text::runs(&p.p_content) {
                for c in &r.run_inner_content {
                    match c {
                        EG_RunInnerContent::Pict(p) => {
                            for e in &p.any {
                                text_box_contents(e, &mut contents);
                            }
                        }
                        EG_RunInnerContent::Drawing(d) => {
                            for choice in &d.choice {
                                let graphic = match choice {
                                    wml::CT_Drawing_Choice::Inline(i) => i.graphic.as_deref(),
                                    wml::CT_Drawing_Choice::Anchor(a) => a.graphic.as_deref(),
                                    _ => None,
                                };
                                for e in graphic
                                    .and_then(|g| g.graphic_data.as_deref())
                                    .map(|d| d.any.as_slice())
                                    .unwrap_or(&[])
                                {
                                    text_box_contents(e, &mut contents);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
        contents
            .iter()
            .map(|c| text::blocks_text(&text::blocks(&c.block_level_elts)))
            .collect()
    }

    /// Puts a diagonal, semi-transparent text watermark (VML WordArt, as
    /// Word does) into every header, creating the default header of the
    /// last section when there is none. Replaces an existing watermark.
    pub fn set_watermark(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Err(Error::InvalidArgument("the watermark text is empty".into()));
        }
        let last = self.sections().len() - 1;
        self.ensure_header_footer(last, HeaderFooterKind::Header, HeaderFooterType::Default, false)?;
        for h in &mut self.headers {
            if h.kind != HeaderFooterKind::Header {
                continue;
            }
            strip_watermarks(&mut h.part.value.block_level_elts);
            self.shared.next_drawing_id += 1;
            let shapes = watermark_shapes(text, self.shared.next_drawing_id)?;
            let blocks = &mut h.part.value.block_level_elts;
            if !blocks.iter().any(|b| matches!(b, EG_BlockLevelElts::P(_))) {
                blocks.push(EG_BlockLevelElts::P(Box::default()));
            }
            let Some(EG_BlockLevelElts::P(p)) =
                blocks.iter_mut().find(|b| matches!(b, EG_BlockLevelElts::P(_)))
            else {
                unreachable!("a paragraph exists")
            };
            p.p_content.push(EG_PContent::R(Box::new(wml::CT_R {
                run_inner_content: vec![pict(shapes)],
                ..Default::default()
            })));
            h.part.dirty = true;
        }
        Ok(())
    }

    /// The text of the watermark, if the headers have one.
    pub fn watermark(&self) -> Option<String> {
        self.headers.iter().find_map(|h| {
            let mut found = None;
            crate::walk::walk_blocks_ref(&h.part.value.block_level_elts, &mut |p| {
                for r in text::runs(&p.p_content) {
                    for c in &r.run_inner_content {
                        if let EG_RunInnerContent::Pict(pict) = c
                            && found.is_none()
                        {
                            found = watermark_text(pict);
                        }
                    }
                }
            });
            found
        })
    }

    /// Removes the watermark from every header. Returns whether one was found.
    pub fn remove_watermark(&mut self) -> bool {
        let mut found = false;
        for h in &mut self.headers {
            if strip_watermarks(&mut h.part.value.block_level_elts) {
                h.part.dirty = true;
                found = true;
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_have_geometry_colors_and_wrapping() {
        let mut options = ShapeOptions::new(Length::inches(1.0), Length::pt(36.5));
        options.fill = Some("#ff0000".into());
        options.position = Some((Length::inches(0.5), Length::ZERO));
        options.hyperlink = Some("https://example.com/?a=1&b=2".into());
        let rect = shape_element(ShapeKind::Rectangle, &options, 1).unwrap();
        assert!(rect.name.is(Ns::V, "rect"));
        assert_eq!(rect.attr(Ns::NONE, "fillcolor"), Some("#FF0000"));
        assert_eq!(rect.attr(Ns::NONE, "href"), Some("https://example.com/?a=1&b=2"));
        let style = rect.attr(Ns::NONE, "style").unwrap();
        assert!(style.contains("position:absolute;margin-left:36pt"), "{style}");
        assert!(style.contains("height:36.5pt"), "{style}");
        assert_eq!(
            rect.child(Ns::W10, "wrap").unwrap().attr(Ns::NONE, "type"),
            Some("square")
        );

        options.wrap = Wrap::BehindText;
        options.stroke = None;
        let oval = shape_element(ShapeKind::Ellipse, &options, 2).unwrap();
        assert!(oval.attr(Ns::NONE, "style").unwrap().contains("z-index:-"));
        assert!(oval.child(Ns::W10, "wrap").is_none());
        assert_eq!(oval.attr(Ns::NONE, "stroked"), Some("f"));
        let line = shape_element(ShapeKind::Line, &options, 3).unwrap();
        assert_eq!(line.attr(Ns::NONE, "to"), Some("72pt,36.5pt"));
        assert!(line.attr(Ns::NONE, "fillcolor").is_none());
        let round = shape_element(
            ShapeKind::RoundedRectangle,
            &ShapeOptions::new(Length::pt(10.0), Length::pt(10.0)),
            4,
        )
        .unwrap();
        assert_eq!(round.attr(Ns::NONE, "arcsize"), Some("10923f"));
        assert!(
            shape_element(
                ShapeKind::Rectangle,
                &ShapeOptions::new(Length::ZERO, Length::pt(1.0)),
                5
            )
            .is_err()
        );
        options.fill = Some("nope".into());
        assert!(shape_element(ShapeKind::Rectangle, &options, 6).is_err());
    }

    #[test]
    fn watermark_markup() {
        let shapes = watermark_shapes("Tom & \"Jerry\"", 3).unwrap();
        assert_eq!(shapes.len(), 2);
        assert!(shapes[0].name.is(Ns::V, "shapetype"));
        let pict = wml::CT_Picture {
            any: shapes,
            ..Default::default()
        };
        assert!(is_watermark(&pict));
        assert_eq!(watermark_text(&pict).as_deref(), Some("Tom & \"Jerry\""));
        let handle = pict.any[0]
            .child(Ns::V, "handles")
            .unwrap()
            .child(Ns::V, "h")
            .unwrap();
        assert_eq!(handle.attr(Ns::NONE, "position"), Some("#0,bottomRight"));
        assert_eq!(pict.any[1].attr(Ns::NONE, "type"), Some("#_x0000_t136"));
    }
}
