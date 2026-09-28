//! Footnotes and endnotes.

use openxml_core::{Error, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::wml::{
    self, EG_BlockLevelElts, EG_PContent, EG_RunInnerContent, ST_FtnEdn, ST_NumberFormat,
};

use crate::document::{Document, Shared, ensure_part};
use crate::markup;
use crate::paragraph::ParagraphMut;
use crate::run::RunMut;
use crate::section::body_sections_mut;
use crate::text;
use crate::util::{decimal, string_val};

/// Footnote or endnote.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteKind {
    /// A footnote (bottom of the page).
    Footnote,
    /// An endnote (end of the section or document).
    Endnote,
}

/// A footnote or endnote read from a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// Note identifier (referenced by `w:footnoteReference`/`w:endnoteReference`).
    pub id: i64,
    /// Text of the note (paragraphs separated by `\n`), without the
    /// reference mark and the space Word puts after it.
    pub text: String,
}

/// Where notes are placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotePosition {
    /// Bottom of the page (footnotes).
    PageBottom,
    /// Directly below the text (footnotes).
    BeneathText,
    /// End of the section (endnotes).
    SectionEnd,
    /// End of the document (endnotes).
    DocumentEnd,
}

/// When note numbering restarts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteRestart {
    /// Numbering continues through the document.
    Continuous,
    /// Numbering restarts in each section.
    EachSection,
    /// Numbering restarts on each page (footnotes).
    EachPage,
}

/// Numbering and placement of notes. Fields left at `None` keep the
/// application default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoteProperties {
    /// Placement.
    pub position: Option<NotePosition>,
    /// Number format of the reference marks.
    pub number_format: Option<ST_NumberFormat>,
    /// First number.
    pub start: Option<u32>,
    /// Restart rule.
    pub restart: Option<NoteRestart>,
}

/// A separator note as Word writes it (`w:separator` or `w:continuationSeparator`).
fn separator(kind: ST_FtnEdn, id: i64) -> wml::CT_FtnEdn {
    let inner = match kind {
        ST_FtnEdn::Separator => EG_RunInnerContent::Separator(Box::default()),
        _ => EG_RunInnerContent::ContinuationSeparator(Box::default()),
    };
    let p = wml::CT_P {
        p_pr: Some(Box::new(wml::CT_PPr {
            spacing: Some(Box::new(wml::CT_Spacing {
                after: Some(openxml_schema::shared_types::ST_TwipsMeasure::UnsignedDecimalNumber(0)),
                line: Some(wml::ST_SignedTwipsMeasure::Integer(240)),
                line_rule: Some(wml::ST_LineSpacingRule::Auto),
                ..Default::default()
            })),
            ..Default::default()
        })),
        p_content: vec![EG_PContent::R(Box::new(wml::CT_R {
            run_inner_content: vec![inner],
            ..Default::default()
        }))],
        ..Default::default()
    };
    wml::CT_FtnEdn {
        type_: Some(kind),
        id: Some(id),
        block_level_elts: vec![EG_BlockLevelElts::P(Box::new(p))],
        ..Default::default()
    }
}

fn separators() -> Vec<wml::CT_FtnEdn> {
    vec![
        separator(ST_FtnEdn::Separator, -1),
        separator(ST_FtnEdn::ContinuationSeparator, 0),
    ]
}

fn separator_refs() -> Vec<wml::CT_FtnEdnSepRef> {
    [-1, 0]
        .into_iter()
        .map(|id| wml::CT_FtnEdnSepRef {
            id: Some(id),
            ..Default::default()
        })
        .collect()
}

fn is_normal(n: &wml::CT_FtnEdn) -> bool {
    matches!(n.type_, None | Some(ST_FtnEdn::Normal))
}

impl Shared {
    /// The notes of a kind, creating the part (with the separator notes
    /// and the matching settings) on first use.
    fn notes_mut(&mut self, kind: NoteKind) -> Result<&mut Vec<wml::CT_FtnEdn>> {
        let created = match kind {
            NoteKind::Footnote => self.footnotes.is_none(),
            NoteKind::Endnote => self.endnotes.is_none(),
        };
        if created {
            let settings = self.settings_mut()?;
            match kind {
                NoteKind::Footnote => {
                    let pr = settings.footnote_pr.get_or_insert_with(Default::default);
                    if pr.footnote.is_empty() {
                        pr.footnote = separator_refs();
                    }
                }
                NoteKind::Endnote => {
                    let pr = settings.endnote_pr.get_or_insert_with(Default::default);
                    if pr.endnote.is_empty() {
                        pr.endnote = separator_refs();
                    }
                }
            }
        }
        Ok(match kind {
            NoteKind::Footnote => {
                &mut ensure_part(
                    &mut self.package,
                    &self.main_part,
                    &mut self.footnotes,
                    "/word/footnotes.xml",
                    "/word/footnotes{}.xml",
                    ct::WML_FOOTNOTES,
                    rel_types::FOOTNOTES,
                    || wml::CT_Footnotes {
                        footnote: separators(),
                        ..Default::default()
                    },
                )?
                .footnote
            }
            NoteKind::Endnote => {
                &mut ensure_part(
                    &mut self.package,
                    &self.main_part,
                    &mut self.endnotes,
                    "/word/endnotes.xml",
                    "/word/endnotes{}.xml",
                    ct::WML_ENDNOTES,
                    rel_types::ENDNOTES,
                    || wml::CT_Endnotes {
                        endnote: separators(),
                        ..Default::default()
                    },
                )?
                .endnote
            }
        })
    }

    /// Adds a note with `text` and returns the reference run to put in the text.
    pub(crate) fn add_note(&mut self, kind: NoteKind, note_text: &str) -> Result<wml::CT_R> {
        let (text_style, ref_style) = match kind {
            NoteKind::Footnote => ("FootnoteText", "FootnoteReference"),
            NoteKind::Endnote => ("EndnoteText", "EndnoteReference"),
        };
        let text_style = self.resolve_style(text_style)?;
        let ref_style = self.resolve_style(ref_style)?;
        let notes = self.notes_mut(kind)?;
        let id = notes.iter().filter_map(|n| n.id).max().unwrap_or(0).max(0) + 1;
        let mut blocks = Vec::new();
        for (i, line) in note_text.split('\n').enumerate() {
            let mut p = wml::CT_P {
                p_pr: Some(Box::new(wml::CT_PPr {
                    p_style: Some(string_val(&text_style)),
                    ..Default::default()
                })),
                ..Default::default()
            };
            let mut content = if i == 0 {
                format!(" {line}")
            } else {
                line.to_owned()
            };
            if i == 0 {
                let mut mark = wml::CT_R::default();
                RunMut::new(&mut mark).style(&ref_style);
                mark.run_inner_content.push(match kind {
                    NoteKind::Footnote => EG_RunInnerContent::FootnoteRef(Box::default()),
                    NoteKind::Endnote => EG_RunInnerContent::EndnoteRef(Box::default()),
                });
                p.p_content.push(EG_PContent::R(Box::new(mark)));
            }
            if !content.trim().is_empty() || i == 0 {
                if content.trim().is_empty() {
                    content = " ".into();
                }
                let mut r = wml::CT_R::default();
                RunMut::new(&mut r).add_text(&content);
                p.p_content.push(EG_PContent::R(Box::new(r)));
            }
            blocks.push(EG_BlockLevelElts::P(Box::new(p)));
        }
        notes.push(wml::CT_FtnEdn {
            id: Some(id),
            block_level_elts: blocks,
            ..Default::default()
        });
        let reference = Box::new(wml::CT_FtnEdnRef {
            id: Some(id),
            ..Default::default()
        });
        let mut r = wml::CT_R::default();
        RunMut::new(&mut r).style(&ref_style);
        r.run_inner_content.push(match kind {
            NoteKind::Footnote => EG_RunInnerContent::FootnoteReference(reference),
            NoteKind::Endnote => EG_RunInnerContent::EndnoteReference(reference),
        });
        Ok(r)
    }
}

impl ParagraphMut<'_> {
    /// Appends a footnote reference mark and adds the footnote with `text`
    /// (lines become paragraphs). Returns the footnote id.
    ///
    /// ```
    /// use openxml_docx::Document;
    ///
    /// let mut doc = Document::new();
    /// let id = doc.add_paragraph("Claim").add_footnote("Source: survey 2024")?;
    /// assert_eq!(doc.footnotes()[0].id, id);
    /// assert_eq!(doc.footnotes()[0].text, "Source: survey 2024");
    /// # Ok::<(), openxml_docx::Error>(())
    /// ```
    pub fn add_footnote(&mut self, text: &str) -> Result<i64> {
        self.add_note(NoteKind::Footnote, text)
    }

    /// Appends an endnote reference mark and adds the endnote. Returns its id.
    pub fn add_endnote(&mut self, text: &str) -> Result<i64> {
        self.add_note(NoteKind::Endnote, text)
    }

    fn add_note(&mut self, kind: NoteKind, text: &str) -> Result<i64> {
        let run = self.shared.add_note(kind, text)?;
        let id = run
            .run_inner_content
            .iter()
            .find_map(|c| match c {
                EG_RunInnerContent::FootnoteReference(r) | EG_RunInnerContent::EndnoteReference(r) => r.id,
                _ => None,
            })
            .expect("the reference run has an id");
        self.p.p_content.push(EG_PContent::R(Box::new(run)));
        Ok(id)
    }
}

fn read_notes(notes: &[wml::CT_FtnEdn]) -> Vec<Note> {
    notes
        .iter()
        .filter(|n| is_normal(n))
        .map(|n| {
            let text = text::blocks_text(&text::blocks(&n.block_level_elts));
            Note {
                id: n.id.unwrap_or_default(),
                text: text.strip_prefix(' ').map(str::to_owned).unwrap_or(text),
            }
        })
        .collect()
}

fn footnote_props(p: &NoteProperties) -> Result<wml::CT_FtnProps> {
    Ok(wml::CT_FtnProps {
        pos: p
            .position
            .map(|pos| {
                Ok::<_, Error>(Box::new(wml::CT_FtnPos {
                    val: Some(match pos {
                        NotePosition::PageBottom => wml::ST_FtnPos::PageBottom,
                        NotePosition::BeneathText => wml::ST_FtnPos::BeneathText,
                        NotePosition::SectionEnd | NotePosition::DocumentEnd => {
                            return Err(Error::InvalidArgument(
                                "footnotes are placed at the page bottom or beneath the text".into(),
                            ));
                        }
                    }),
                    ..Default::default()
                }))
            })
            .transpose()?,
        num_fmt: number_format(p),
        num_start: p.start.map(|s| decimal(i64::from(s))),
        num_restart: restart(p),
        ..Default::default()
    })
}

fn endnote_props(p: &NoteProperties) -> Result<wml::CT_EdnProps> {
    if p.restart == Some(NoteRestart::EachPage) {
        return Err(Error::InvalidArgument(
            "endnote numbering cannot restart on each page".into(),
        ));
    }
    Ok(wml::CT_EdnProps {
        pos: p
            .position
            .map(|pos| {
                Ok::<_, Error>(Box::new(wml::CT_EdnPos {
                    val: Some(match pos {
                        NotePosition::SectionEnd => wml::ST_EdnPos::SectEnd,
                        NotePosition::DocumentEnd => wml::ST_EdnPos::DocEnd,
                        NotePosition::PageBottom | NotePosition::BeneathText => {
                            return Err(Error::InvalidArgument(
                                "endnotes are placed at the end of the section or document".into(),
                            ));
                        }
                    }),
                    ..Default::default()
                }))
            })
            .transpose()?,
        num_fmt: number_format(p),
        num_start: p.start.map(|s| decimal(i64::from(s))),
        num_restart: restart(p),
        ..Default::default()
    })
}

fn number_format(p: &NoteProperties) -> Option<Box<wml::CT_NumFmt>> {
    p.number_format.map(|f| {
        Box::new(wml::CT_NumFmt {
            val: Some(f),
            ..Default::default()
        })
    })
}

fn restart(p: &NoteProperties) -> Option<Box<wml::CT_NumRestart>> {
    p.restart.map(|r| {
        Box::new(wml::CT_NumRestart {
            val: Some(match r {
                NoteRestart::Continuous => wml::ST_RestartNumber::Continuous,
                NoteRestart::EachSection => wml::ST_RestartNumber::EachSect,
                NoteRestart::EachPage => wml::ST_RestartNumber::EachPage,
            }),
            ..Default::default()
        })
    })
}

fn read_properties(
    pos: Option<NotePosition>,
    fmt: &Option<Box<wml::CT_NumFmt>>,
    start: &Option<Box<wml::CT_DecimalNumber>>,
    restart: &Option<Box<wml::CT_NumRestart>>,
) -> NoteProperties {
    NoteProperties {
        position: pos,
        number_format: fmt.as_deref().and_then(|f| f.val),
        start: start.as_deref().and_then(|s| s.val).map(|v| v.max(0) as u32),
        restart: restart.as_deref().and_then(|r| r.val).map(|r| match r {
            wml::ST_RestartNumber::Continuous => NoteRestart::Continuous,
            wml::ST_RestartNumber::EachSect => NoteRestart::EachSection,
            wml::ST_RestartNumber::EachPage => NoteRestart::EachPage,
        }),
    }
}

impl Document {
    fn note_list(&self, kind: NoteKind) -> Vec<Note> {
        match kind {
            NoteKind::Footnote => self
                .shared
                .footnotes
                .as_ref()
                .map(|f| read_notes(&f.value.footnote))
                .unwrap_or_default(),
            NoteKind::Endnote => self
                .shared
                .endnotes
                .as_ref()
                .map(|f| read_notes(&f.value.endnote))
                .unwrap_or_default(),
        }
    }

    /// Footnotes of the document (separator notes excluded).
    pub fn footnotes(&self) -> Vec<Note> {
        self.note_list(NoteKind::Footnote)
    }

    /// Endnotes of the document (separator notes excluded).
    pub fn endnotes(&self) -> Vec<Note> {
        self.note_list(NoteKind::Endnote)
    }

    /// Removes a footnote and its reference marks.
    pub fn remove_footnote(&mut self, id: i64) -> Result<()> {
        self.remove_note(NoteKind::Footnote, id)
    }

    /// Removes an endnote and its reference marks.
    pub fn remove_endnote(&mut self, id: i64) -> Result<()> {
        self.remove_note(NoteKind::Endnote, id)
    }

    fn remove_note(&mut self, kind: NoteKind, id: i64) -> Result<()> {
        let removed = match kind {
            NoteKind::Footnote => self.shared.footnotes.as_mut().is_some_and(|t| {
                let before = t.value.footnote.len();
                t.value.footnote.retain(|n| !(n.id == Some(id) && is_normal(n)));
                t.dirty |= t.value.footnote.len() != before;
                t.value.footnote.len() != before
            }),
            NoteKind::Endnote => self.shared.endnotes.as_mut().is_some_and(|t| {
                let before = t.value.endnote.len();
                t.value.endnote.retain(|n| !(n.id == Some(id) && is_normal(n)));
                t.dirty |= t.value.endnote.len() != before;
                t.value.endnote.len() != before
            }),
        };
        if !removed {
            return Err(Error::NotFound(format!("{kind:?} {id}").to_lowercase()));
        }
        let mut edit_run = |r: &mut wml::CT_R| {
            let before = r.run_inner_content.len();
            r.run_inner_content.retain(|c| {
                !matches!((kind, c),
                    (NoteKind::Footnote, EG_RunInnerContent::FootnoteReference(n))
                    | (NoteKind::Endnote, EG_RunInnerContent::EndnoteReference(n)) if n.id == Some(id))
            });
            r.run_inner_content.len() != before
        };
        if let Some(body) = self.main.body.as_mut()
            && markup::retain_in_blocks(&mut body.block_level_elts, &|_| false, &|_| false, &mut edit_run)
        {
            self.main_dirty = true;
        }
        Ok(())
    }

    /// Sets the numbering and placement of footnotes for the whole document
    /// (in the settings and in every section, where Word reads them).
    pub fn set_footnote_properties(&mut self, props: &NoteProperties) -> Result<()> {
        let section = footnote_props(props)?;
        let settings = self.shared.settings_mut()?;
        let pr = settings.footnote_pr.get_or_insert_with(Default::default);
        pr.pos = section.pos.clone();
        pr.num_fmt = section.num_fmt.clone();
        pr.num_start = section.num_start.clone();
        pr.num_restart = section.num_restart.clone();
        for sect in body_sections_mut(self.body_mut()) {
            sect.footnote_pr = Some(Box::new(section.clone()));
        }
        Ok(())
    }

    /// Sets the numbering and placement of endnotes for the whole document.
    pub fn set_endnote_properties(&mut self, props: &NoteProperties) -> Result<()> {
        let section = endnote_props(props)?;
        let settings = self.shared.settings_mut()?;
        let pr = settings.endnote_pr.get_or_insert_with(Default::default);
        pr.pos = section.pos.clone();
        pr.num_fmt = section.num_fmt.clone();
        pr.num_start = section.num_start.clone();
        pr.num_restart = section.num_restart.clone();
        for sect in body_sections_mut(self.body_mut()) {
            sect.endnote_pr = Some(Box::new(section.clone()));
        }
        Ok(())
    }

    /// Document-wide footnote properties (from the settings).
    pub fn footnote_properties(&self) -> NoteProperties {
        let Some(pr) = self.settings().and_then(|s| s.footnote_pr.as_deref()) else {
            return NoteProperties::default();
        };
        let pos = pr.pos.as_deref().and_then(|p| p.val).map(|p| match p {
            wml::ST_FtnPos::PageBottom => NotePosition::PageBottom,
            wml::ST_FtnPos::BeneathText => NotePosition::BeneathText,
            wml::ST_FtnPos::SectEnd => NotePosition::SectionEnd,
            wml::ST_FtnPos::DocEnd => NotePosition::DocumentEnd,
        });
        read_properties(pos, &pr.num_fmt, &pr.num_start, &pr.num_restart)
    }

    /// Document-wide endnote properties (from the settings).
    pub fn endnote_properties(&self) -> NoteProperties {
        let Some(pr) = self.settings().and_then(|s| s.endnote_pr.as_deref()) else {
            return NoteProperties::default();
        };
        let pos = pr.pos.as_deref().and_then(|p| p.val).map(|p| match p {
            wml::ST_EdnPos::SectEnd => NotePosition::SectionEnd,
            wml::ST_EdnPos::DocEnd => NotePosition::DocumentEnd,
        });
        read_properties(pos, &pr.num_fmt, &pr.num_start, &pr.num_restart)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_note_parts_have_word_separators() {
        let mut doc = Document::new();
        let run = doc.shared.add_note(NoteKind::Footnote, "Text\nMore").unwrap();
        assert!(matches!(
            run.run_inner_content[0],
            EG_RunInnerContent::FootnoteReference(_)
        ));
        let notes = &doc.shared.footnotes.as_ref().unwrap().value.footnote;
        assert_eq!(notes.len(), 3);
        assert_eq!(
            (notes[0].type_, notes[0].id),
            (Some(ST_FtnEdn::Separator), Some(-1))
        );
        assert_eq!(
            (notes[1].type_, notes[1].id),
            (Some(ST_FtnEdn::ContinuationSeparator), Some(0))
        );
        assert_eq!(notes[2].id, Some(1));
        assert_eq!(notes[2].block_level_elts.len(), 2);
        let refs = &doc.settings().unwrap().footnote_pr.as_ref().unwrap().footnote;
        assert_eq!(refs.iter().map(|r| r.id).collect::<Vec<_>>(), [Some(-1), Some(0)]);
        assert_eq!(
            doc.footnotes(),
            [Note {
                id: 1,
                text: "Text\nMore".into()
            }]
        );
        assert!(doc.endnotes().is_empty());
    }

    #[test]
    fn note_properties_are_validated() {
        let bad = NoteProperties {
            position: Some(NotePosition::DocumentEnd),
            ..Default::default()
        };
        assert!(footnote_props(&bad).is_err());
        assert!(endnote_props(&bad).is_ok());
        let bad = NoteProperties {
            position: Some(NotePosition::PageBottom),
            ..Default::default()
        };
        assert!(endnote_props(&bad).is_err());
        let bad = NoteProperties {
            restart: Some(NoteRestart::EachPage),
            ..Default::default()
        };
        assert!(endnote_props(&bad).is_err());
        assert!(footnote_props(&bad).is_ok());
    }
}
