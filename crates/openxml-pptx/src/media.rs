//! Embedded audio and video, stored the way PowerPoint 2010+ does: a
//! `p:pic` whose `p:nvPr` holds `a:videoFile`/`a:audioFile` (`r:link` to a
//! video/audio relationship) plus a `p14:media` extension (`r:embed` to a
//! media relationship), a poster image, and playback nodes in the timing tree.

use openxml_core::image::tiny_png;
use openxml_core::{Error, Length, Result};
use openxml_opc::PartName;
use openxml_opc::known::rel_types;
use openxml_schema::{dml, pml};
use openxml_xml::{Ns, RawElement};

use crate::animation;
use crate::picture;
use crate::presentation::Presentation;
use crate::shape;
use crate::slide::SlideMut;
use crate::util;

/// Relationship type of the `p14:media` embedding (Office 2010 and later).
pub const MEDIA_REL_TYPE: &str = "http://schemas.microsoft.com/office/2007/relationships/media";
/// URI of the `p:ext` holding `p14:media`.
const MEDIA_EXT_URI: &str = "{DAA4B4D4-6D71-4841-9C94-3DA8F4ED2E4C}";
const P14_NS: &str = "http://schemas.microsoft.com/office/powerpoint/2010/main";

/// Whether a clip is audio or video.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// Video.
    Video,
    /// Audio.
    Audio,
}

/// A recognised media container.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MediaFormat {
    /// File extension.
    pub extension: &'static str,
    /// MIME content type.
    pub content_type: &'static str,
    /// Audio or video.
    pub kind: MediaKind,
}

const fn format(extension: &'static str, content_type: &'static str, kind: MediaKind) -> MediaFormat {
    MediaFormat {
        extension,
        content_type,
        kind,
    }
}

/// Recognises common media containers by their signature. ASF files
/// (WMV/WMA) and MPEG-4 files without a telling brand are classified with
/// the help of `hint`.
pub fn sniff_media(bytes: &[u8], hint: MediaKind) -> Option<MediaFormat> {
    let starts = |sig: &[u8]| bytes.starts_with(sig);
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some(match &bytes[8..12] {
            b"qt  " => format("mov", "video/quicktime", MediaKind::Video),
            b"M4A " | b"M4B " => format("m4a", "audio/mp4", MediaKind::Audio),
            b"M4V " | b"M4VH" | b"M4VP" => format("m4v", "video/x-m4v", MediaKind::Video),
            _ if hint == MediaKind::Audio => format("m4a", "audio/mp4", MediaKind::Audio),
            _ => format("mp4", "video/mp4", MediaKind::Video),
        });
    }
    if bytes.len() >= 12 && starts(b"RIFF") {
        return match &bytes[8..12] {
            b"WAVE" => Some(format("wav", "audio/wav", MediaKind::Audio)),
            b"AVI " => Some(format("avi", "video/x-msvideo", MediaKind::Video)),
            _ => None,
        };
    }
    if starts(b"ID3") || (bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0) {
        return Some(format("mp3", "audio/mpeg", MediaKind::Audio));
    }
    if starts(&[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
        return Some(match hint {
            MediaKind::Audio => format("wma", "audio/x-ms-wma", MediaKind::Audio),
            MediaKind::Video => format("wmv", "video/x-ms-wmv", MediaKind::Video),
        });
    }
    if starts(&[0x00, 0x00, 0x01, 0xBA]) || starts(&[0x00, 0x00, 0x01, 0xB3]) {
        return Some(format("mpg", "video/mpeg", MediaKind::Video));
    }
    None
}

/// A media clip found on a slide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaInfo {
    /// The picture shape presenting the clip.
    pub shape_id: u32,
    /// Audio or video.
    pub kind: MediaKind,
    /// The embedded media part, when the clip is embedded.
    pub part: Option<PartName>,
    /// Content type of the media part.
    pub content_type: Option<String>,
}

fn media_ext(r_id: &str) -> pml::CT_Extension {
    let raw = RawElement::parse(&format!(
        r#"<p14:media xmlns:p14="{P14_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:embed="{r_id}"/>"#
    ))
    .expect("well-formed");
    pml::CT_Extension {
        uri: Some(MEDIA_EXT_URI.to_owned()),
        any: vec![raw],
        ..Default::default()
    }
}

/// `r:embed` of the `p14:media` extension of a picture, if any.
fn embedded_media_rel(nv_pr: &pml::CT_ApplicationNonVisualDrawingProps) -> Option<String> {
    nv_pr
        .ext_lst
        .as_ref()?
        .ext
        .iter()
        .filter(|e| e.uri.as_deref() == Some(MEDIA_EXT_URI))
        .flat_map(|e| &e.any)
        .find(|raw| &*raw.name.local == "media")
        .and_then(|raw| raw.attr(Ns::R, "embed").map(str::to_owned))
}

/// Timing nodes PowerPoint writes for a clip: the media node (paused until
/// started) and an interactive sequence toggling playback when the clip is clicked.
fn add_media_timing(slide: &mut pml::CT_Slide, spid: u32, kind: MediaKind) {
    let (root, mut ids) = animation::root_list(slide);
    let tgt = format!(r#"<p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl>"#);
    let node = match kind {
        MediaKind::Video => format!(
            concat!(
                r#"<p:video><p:cMediaNode vol="80000"><p:cTn id="{id}" fill="hold" display="0">"#,
                r#"<p:stCondLst><p:cond delay="indefinite"/></p:stCondLst></p:cTn>{tgt}</p:cMediaNode></p:video>"#
            ),
            id = ids.next(),
            tgt = tgt
        ),
        MediaKind::Audio => format!(
            concat!(
                r#"<p:audio><p:cMediaNode vol="80000"><p:cTn id="{id}" fill="hold" display="0">"#,
                r#"<p:stCondLst><p:cond delay="indefinite"/></p:stCondLst>"#,
                r#"<p:endCondLst><p:cond evt="onStopAudio" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:endCondLst>"#,
                r#"</p:cTn>{tgt}</p:cMediaNode></p:audio>"#
            ),
            id = ids.next(),
            tgt = tgt
        ),
    };
    let seq = format!(
        concat!(
            r#"<p:seq concurrent="1" nextAc="seek"><p:cTn id="{a}" restart="whenNotActive" fill="hold" "#,
            r#"evtFilter="cancelBubble" nodeType="interactiveSeq"><p:stCondLst><p:cond evt="onClick" delay="0">{tgt}</p:cond>"#,
            r#"</p:stCondLst><p:endSync evt="end" delay="0"><p:rtn val="all"/></p:endSync><p:childTnLst>"#,
            r#"<p:par><p:cTn id="{b}" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>"#,
            r#"<p:par><p:cTn id="{c}" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>"#,
            r#"<p:par><p:cTn id="{d}" presetID="2" presetClass="mediacall" presetSubtype="0" fill="hold" nodeType="clickEffect">"#,
            r#"<p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>"#,
            r#"<p:cmd type="call" cmd="togglePause"><p:cBhvr><p:cTn id="{e}" dur="1" fill="hold"/>{tgt}</p:cBhvr></p:cmd>"#,
            r#"</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>"#,
            r#"</p:childTnLst></p:cTn><p:nextCondLst><p:cond evt="onClick" delay="0">{tgt}</p:cond></p:nextCondLst></p:seq>"#
        ),
        a = ids.next(),
        b = ids.next(),
        c = ids.next(),
        d = ids.next(),
        e = ids.next(),
        tgt = tgt
    );
    let node: pml::CT_TimeNodeList = util::fragment(&format!("<p:childTnLst>{node}{seq}</p:childTnLst>"));
    root.choice.extend(node.choice);
}

impl SlideMut<'_> {
    /// Embeds a video clip shown in the given frame. `poster` is the image
    /// shown before playback (PNG, JPEG, …; a plain frame when `None`).
    /// Clicking the clip plays and pauses it. Returns the shape identifier.
    ///
    /// ```
    /// use openxml_core::Length;
    /// use openxml_pptx::{LayoutKind, MediaKind, Presentation};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// let mut mp4 = vec![0, 0, 0, 24];
    /// mp4.extend_from_slice(b"ftypisom\0\0\x02\0isomiso2mp41");
    /// let id = slide.add_video(&mp4, None, Length::cm(2.0), Length::cm(2.0), Length::cm(16.0), Length::cm(9.0))?;
    /// let media = deck.slide_media(0);
    /// assert_eq!((media[0].shape_id, media[0].kind), (id, MediaKind::Video));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn add_video(
        &mut self,
        video: &[u8],
        poster: Option<&[u8]>,
        x: Length,
        y: Length,
        w: Length,
        h: Length,
    ) -> Result<u32> {
        self.add_media(video, MediaKind::Video, poster, x, y, w, h)
    }

    /// Embeds an audio clip, shown as its poster image (a small plain icon
    /// when `None`). Returns the shape identifier.
    pub fn add_audio(
        &mut self,
        audio: &[u8],
        poster: Option<&[u8]>,
        x: Length,
        y: Length,
        w: Length,
        h: Length,
    ) -> Result<u32> {
        self.add_media(audio, MediaKind::Audio, poster, x, y, w, h)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_media(
        &mut self,
        bytes: &[u8],
        kind: MediaKind,
        poster: Option<&[u8]>,
        x: Length,
        y: Length,
        w: Length,
        h: Length,
    ) -> Result<u32> {
        let format = sniff_media(bytes, kind)
            .filter(|f| f.kind == kind)
            .ok_or_else(|| Error::InvalidArgument(format!("unrecognised {kind:?} format")))?;
        let default_poster;
        let poster = match poster {
            Some(p) => p,
            None => {
                default_poster = tiny_png(16, 9);
                &default_poster
            }
        };
        let slide_part = self.part.clone();
        let pkg = &mut self.pres.package;
        let media = picture::relate_media_part(
            pkg,
            bytes,
            &format!("/ppt/media/media{{}}.{}", format.extension),
            format.content_type,
        )?;
        let media_rid = picture::relate(pkg, &slide_part, MEDIA_REL_TYPE, &media)?;
        let link_type = match kind {
            MediaKind::Video => rel_types::VIDEO,
            MediaKind::Audio => rel_types::AUDIO,
        };
        let link_rid = picture::relate(pkg, &slide_part, link_type, &media)?;
        let (poster_rid, _) = picture::relate_image(pkg, &slide_part, poster)?;

        let id = self.next_id();
        let name = match kind {
            MediaKind::Video => format!("Video {}", id - 1),
            MediaKind::Audio => format!("Audio {}", id - 1),
        };
        let mut pic = picture::new_picture(id, &name, poster_rid, Some(shape::transform(x, y, w, h)), None);
        {
            let nv = pic.nv_pic_pr.as_mut().expect("built with properties");
            nv.c_nv_pr.as_mut().expect("built").hlink_click = Some(Box::new(dml::CT_Hyperlink {
                r_id: Some(String::new()),
                action: Some("ppaction://media".into()),
                ..Default::default()
            }));
            let nv_pr = nv.nv_pr.get_or_insert_with(Box::default);
            nv_pr.media = Some(match kind {
                MediaKind::Video => dml::EG_Media::VideoFile(Box::new(dml::CT_VideoFile {
                    r_link: Some(link_rid),
                    ..Default::default()
                })),
                MediaKind::Audio => dml::EG_Media::AudioFile(Box::new(dml::CT_AudioFile {
                    r_link: Some(link_rid),
                    ..Default::default()
                })),
            });
            nv_pr.ext_lst = Some(Box::new(pml::CT_ExtensionList {
                ext: vec![media_ext(&media_rid)],
                ..Default::default()
            }));
        }
        self.tree_mut()
            .choice
            .push(pml::CT_GroupShape_Choice::Pic(Box::new(pic)));
        add_media_timing(self.raw_mut(), id, kind);
        Ok(id)
    }
}

impl Presentation {
    /// The audio and video clips of the slide at `index`.
    pub fn slide_media(&self, index: usize) -> Vec<MediaInfo> {
        let Some(slide) = self.slides.get(index) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let Some(tree) = slide.data.c_sld.as_ref().and_then(|c| c.sp_tree.as_deref()) else {
            return out;
        };
        fn walk(tree: &pml::CT_GroupShape, out: &mut Vec<(u32, MediaKind, Option<String>)>) {
            for c in &tree.choice {
                match c {
                    pml::CT_GroupShape_Choice::Pic(p) => {
                        let Some(nv) = p.nv_pic_pr.as_ref() else { continue };
                        let Some(nv_pr) = nv.nv_pr.as_deref() else {
                            continue;
                        };
                        let (kind, link) = match nv_pr.media.as_ref() {
                            Some(dml::EG_Media::VideoFile(v)) => (MediaKind::Video, v.r_link.clone()),
                            Some(dml::EG_Media::QuickTimeFile(_)) => (MediaKind::Video, None),
                            Some(dml::EG_Media::AudioFile(a)) => (MediaKind::Audio, a.r_link.clone()),
                            Some(dml::EG_Media::WavAudioFile(w)) => (MediaKind::Audio, w.r_embed.clone()),
                            _ => continue,
                        };
                        let rel = embedded_media_rel(nv_pr).or(link);
                        let id = nv.c_nv_pr.as_ref().and_then(|c| c.id).unwrap_or(0);
                        out.push((id, kind, rel));
                    }
                    pml::CT_GroupShape_Choice::GrpSp(g) => walk(g, out),
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        walk(tree, &mut found);
        for (shape_id, kind, rel) in found {
            let part = rel.and_then(|r| self.package.relationship_target(Some(&slide.part), &r));
            let content_type = part
                .as_ref()
                .and_then(|p| self.package.part(p))
                .map(|p| p.content_type().to_owned());
            out.push(MediaInfo {
                shape_id,
                kind,
                part,
                content_type,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_signatures() {
        let ftyp = |brand: &[u8; 4]| {
            let mut v = vec![0, 0, 0, 20];
            v.extend_from_slice(b"ftyp");
            v.extend_from_slice(brand);
            v.extend_from_slice(&[0; 8]);
            v
        };
        assert_eq!(
            sniff_media(&ftyp(b"isom"), MediaKind::Video).unwrap().extension,
            "mp4"
        );
        assert_eq!(
            sniff_media(&ftyp(b"isom"), MediaKind::Audio).unwrap().extension,
            "m4a"
        );
        assert_eq!(
            sniff_media(&ftyp(b"qt  "), MediaKind::Video)
                .unwrap()
                .content_type,
            "video/quicktime"
        );
        assert_eq!(
            sniff_media(&ftyp(b"M4A "), MediaKind::Video).unwrap().kind,
            MediaKind::Audio
        );
        assert_eq!(
            sniff_media(&ftyp(b"M4V "), MediaKind::Video).unwrap().extension,
            "m4v"
        );
        assert_eq!(
            sniff_media(b"RIFF\0\0\0\0WAVEfmt ", MediaKind::Audio)
                .unwrap()
                .extension,
            "wav"
        );
        assert_eq!(
            sniff_media(b"RIFF\0\0\0\0AVI LIST", MediaKind::Video)
                .unwrap()
                .extension,
            "avi"
        );
        assert_eq!(sniff_media(b"RIFF\0\0\0\0XXXX", MediaKind::Video), None);
        assert_eq!(
            sniff_media(b"ID3\x04\0\0\0\0", MediaKind::Audio)
                .unwrap()
                .content_type,
            "audio/mpeg"
        );
        assert_eq!(
            sniff_media(&[0xFF, 0xFB, 0x90, 0], MediaKind::Audio)
                .unwrap()
                .extension,
            "mp3"
        );
        let asf = [0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0];
        assert_eq!(sniff_media(&asf, MediaKind::Audio).unwrap().extension, "wma");
        assert_eq!(sniff_media(&asf, MediaKind::Video).unwrap().extension, "wmv");
        assert_eq!(
            sniff_media(&[0, 0, 1, 0xBA, 0], MediaKind::Video)
                .unwrap()
                .extension,
            "mpg"
        );
        assert_eq!(sniff_media(b"hello", MediaKind::Video), None);
    }

    #[test]
    fn media_extension_round_trip() {
        let ext = media_ext("rId7");
        let nv_pr = pml::CT_ApplicationNonVisualDrawingProps {
            ext_lst: Some(Box::new(pml::CT_ExtensionList {
                ext: vec![ext],
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(embedded_media_rel(&nv_pr).as_deref(), Some("rId7"));
        assert_eq!(
            embedded_media_rel(&pml::CT_ApplicationNonVisualDrawingProps::default()),
            None
        );
    }
}
