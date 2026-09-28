//! Pictures (DrawingML `pic:pic`); the frames are built by [`crate::drawing`].

#[cfg(test)]
use openxml_core::Length;
#[cfg(test)]
use openxml_schema::{dml_picture, wml};

/// URI identifying picture content in `a:graphicData`.
pub(crate) const PICTURE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

/// Builds the `w:drawing` element of an inline picture.
///
/// * `rel_id` — relationship id of the image part;
/// * `id` — drawing object id, unique within the document;
/// * `name` — file name shown in the object's properties.
#[cfg(test)]
pub(crate) fn inline_picture(
    rel_id: &str,
    id: u32,
    name: &str,
    width: Length,
    height: Length,
) -> wml::CT_Drawing {
    let info = crate::drawing::FrameInfo {
        id,
        name: format!("Picture {id}"),
        description: Some(name.to_owned()),
        ..Default::default()
    };
    let data = crate::drawing::picture_data(rel_id, name, &info, width, height);
    crate::drawing::drawing(data, width, height, &info, None)
}

/// Largest drawing object id (`wp:docPr/@id`) used in a part's XML.
///
/// Works on the raw bytes so that drawings nested anywhere (text boxes,
/// alternate content, …) are taken into account.
pub(crate) fn max_drawing_id(xml: &[u8]) -> u32 {
    let mut max = 0u32;
    let mut i = 0;
    while let Some(off) = find(&xml[i..], b"docPr") {
        let start = i + off + 5;
        i = start;
        // Must be an element name: preceded by '<' or ':' and followed by whitespace.
        let before = xml.get(start.wrapping_sub(6)).copied();
        if !matches!(before, Some(b'<' | b':')) || !xml.get(start).is_some_and(|c| c.is_ascii_whitespace()) {
            continue;
        }
        let end = xml[start..]
            .iter()
            .position(|&c| c == b'>')
            .map_or(xml.len(), |p| start + p);
        let tag = &xml[start..end];
        if let Some(p) = find(tag, b" id=\"")
            .or_else(|| find(tag, b"\tid=\""))
            .or_else(|| find(tag, b"\nid=\""))
        {
            let digits: Vec<u8> = tag[p + 5..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .copied()
                .collect();
            if let Some(v) = std::str::from_utf8(&digits)
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
            {
                max = max.max(v);
            }
        }
    }
    max
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_inline_picture_with_extent_and_relationship() {
        let d = inline_picture("rId7", 3, "logo.png", Length::inches(2.0), Length::inches(1.0));
        let wml::CT_Drawing_Choice::Inline(inline) = &d.choice[0] else {
            panic!()
        };
        assert_eq!(inline.extent.as_ref().unwrap().cx, Some(1_828_800));
        assert_eq!(inline.extent.as_ref().unwrap().cy, Some(914_400));
        assert_eq!(inline.doc_pr.as_ref().unwrap().id, Some(3));
        let data = inline.graphic.as_ref().unwrap().graphic_data.as_ref().unwrap();
        assert_eq!(data.uri.as_deref(), Some(PICTURE_URI));
        let pic: dml_picture::CT_Picture = data.any[0].to_typed().unwrap();
        let blip = pic.blip_fill.unwrap().blip.unwrap();
        assert_eq!(blip.r_embed.as_deref(), Some("rId7"));
    }

    #[test]
    fn scans_drawing_ids() {
        let xml = br#"<w:p><wp:docPr id="5" name="a"/><wp:docPr name="b" id="12"/><x:docPrX id="99"/><docPr id="7"/></w:p>"#;
        assert_eq!(max_drawing_id(xml), 12);
        assert_eq!(max_drawing_id(b"<none/>"), 0);
        assert_eq!(max_drawing_id(b"docPr"), 0);
    }
}
