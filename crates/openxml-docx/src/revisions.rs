//! Tracked changes (revisions): creating insertions, deletions and
//! formatting changes, listing them, and accepting or rejecting them.

use openxml_core::{Error, Result};
use openxml_opc::w3cdtf_now;
use openxml_schema::shared_math as m;
use openxml_schema::wml::{
    self, CT_RunTrackChange_Choice as TC, EG_BlockLevelElts, EG_CellMarkupElements, EG_ContentBlockContent,
    EG_ContentRowContent, EG_PContent, EG_RunInnerContent,
};

use crate::document::Document;
use crate::format::convert;
use crate::markup::{self, TextSpan};
use crate::paragraph::ParagraphMut;
use crate::run::RunMut;
use crate::text;
use crate::util::{is_on, on, text_node};

/// Author and date recorded with a tracked change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RevisionInfo {
    /// Author name.
    pub author: String,
    /// Date and time (`YYYY-MM-DDThh:mm:ssZ`); the current time when `None`.
    pub date: Option<String>,
}

impl RevisionInfo {
    /// A change by `author`, dated now.
    pub fn new(author: &str) -> Self {
        RevisionInfo {
            author: author.to_owned(),
            date: None,
        }
    }

    fn date(&self) -> String {
        self.date.clone().unwrap_or_else(w3cdtf_now)
    }
}

/// Kind of a tracked change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RevisionKind {
    /// Inserted content (`w:ins`).
    Insertion,
    /// Deleted content (`w:del`).
    Deletion,
    /// Content moved away from here (`w:moveFrom`).
    MoveFrom,
    /// Content moved here (`w:moveTo`).
    MoveTo,
    /// Changed character formatting (`w:rPrChange`).
    RunFormatting,
    /// Changed paragraph formatting (`w:pPrChange`).
    ParagraphFormatting,
    /// Inserted paragraph mark.
    ParagraphInsertion,
    /// Deleted paragraph mark (the paragraph merges with the next one).
    ParagraphDeletion,
    /// Changed section properties (`w:sectPrChange`).
    SectionFormatting,
    /// Changed table, row or cell properties.
    TableFormatting,
    /// Inserted table row.
    RowInsertion,
    /// Deleted table row.
    RowDeletion,
}

/// A tracked change found in the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    /// Revision identifier.
    pub id: i64,
    /// Kind of change.
    pub kind: RevisionKind,
    /// Author.
    pub author: Option<String>,
    /// Date.
    pub date: Option<String>,
    /// Text concerned by the change (inserted or deleted text, or the text
    /// of the reformatted run); empty for property changes.
    pub text: String,
}

/// Text of a run including deleted text.
fn revision_run_text(r: &wml::CT_R) -> String {
    let deleted: String = r
        .run_inner_content
        .iter()
        .filter_map(|c| match c {
            EG_RunInnerContent::DelText(t) => Some(t.value.as_str()),
            _ => None,
        })
        .collect();
    text::run_text(r) + &deleted
}

fn choices_text(items: &[TC]) -> String {
    let mut s = String::new();
    for c in items {
        match c {
            TC::WR(r) => s.push_str(&revision_run_text(r)),
            TC::Ins(x) | TC::Del(x) | TC::MoveFrom(x) | TC::MoveTo(x) => s.push_str(&choices_text(&x.choice)),
            TC::Sdt(x) => {
                if let Some(c) = &x.sdt_content {
                    s.push_str(&content_text(&c.p_content));
                }
            }
            TC::CustomXml(x) => s.push_str(&content_text(&x.p_content)),
            TC::SmartTag(x) => s.push_str(&content_text(&x.p_content)),
            _ => {}
        }
    }
    s
}

fn content_text(items: &[EG_PContent]) -> String {
    let mut s = String::new();
    for item in items {
        match item {
            EG_PContent::R(r) => s.push_str(&revision_run_text(r)),
            EG_PContent::Ins(x) | EG_PContent::Del(x) | EG_PContent::MoveFrom(x) | EG_PContent::MoveTo(x) => {
                s.push_str(&choices_text(&x.choice))
            }
            _ => s.push_str(&text::paragraph_text(&wml::CT_P {
                p_content: vec![item.clone()],
                ..Default::default()
            })),
        }
    }
    s
}

/// Turns deleted text back into ordinary text (when a deletion is rejected).
fn undelete_run(r: &mut wml::CT_R) {
    for c in &mut r.run_inner_content {
        match c {
            EG_RunInnerContent::DelText(t) => *c = EG_RunInnerContent::T(t.clone()),
            EG_RunInnerContent::DelInstrText(t) => *c = EG_RunInnerContent::InstrText(t.clone()),
            _ => {}
        }
    }
}

/// Turns text into deleted text (for a tracked deletion).
fn delete_run(r: &mut wml::CT_R) {
    for c in &mut r.run_inner_content {
        match c {
            EG_RunInnerContent::T(t) => *c = EG_RunInnerContent::DelText(t.clone()),
            EG_RunInnerContent::InstrText(t) => *c = EG_RunInnerContent::DelInstrText(t.clone()),
            _ => {}
        }
    }
}

fn undelete_choices(items: &mut [TC]) {
    for c in items {
        match c {
            TC::WR(r) => undelete_run(r),
            TC::Ins(x) | TC::Del(x) | TC::MoveFrom(x) | TC::MoveTo(x) => undelete_choices(&mut x.choice),
            _ => {}
        }
    }
}

/// Converts the content of a run-level change into paragraph content.
/// Math elements are gathered into `m:oMath` wrappers.
fn choices_to_content(items: Vec<TC>) -> Vec<EG_PContent> {
    let mut out = Vec::new();
    let mut math: Vec<m::EG_OMathElements> = Vec::new();
    let flush = |math: &mut Vec<m::EG_OMathElements>, out: &mut Vec<EG_PContent>| {
        if !math.is_empty() {
            out.push(EG_PContent::OMath(Box::new(m::CT_OMath {
                o_math_elements: std::mem::take(math),
                ..Default::default()
            })));
        }
    };
    for item in items {
        let math_item = match item {
            TC::Acc(x) => Some(m::EG_OMathElements::Acc(x)),
            TC::Bar(x) => Some(m::EG_OMathElements::Bar(x)),
            TC::Box(x) => Some(m::EG_OMathElements::Box(x)),
            TC::BorderBox(x) => Some(m::EG_OMathElements::BorderBox(x)),
            TC::D(x) => Some(m::EG_OMathElements::D(x)),
            TC::EqArr(x) => Some(m::EG_OMathElements::EqArr(x)),
            TC::F(x) => Some(m::EG_OMathElements::F(x)),
            TC::Func(x) => Some(m::EG_OMathElements::Func(x)),
            TC::GroupChr(x) => Some(m::EG_OMathElements::GroupChr(x)),
            TC::LimLow(x) => Some(m::EG_OMathElements::LimLow(x)),
            TC::LimUpp(x) => Some(m::EG_OMathElements::LimUpp(x)),
            TC::M(x) => Some(m::EG_OMathElements::M(x)),
            TC::Nary(x) => Some(m::EG_OMathElements::Nary(x)),
            TC::Phant(x) => Some(m::EG_OMathElements::Phant(x)),
            TC::Rad(x) => Some(m::EG_OMathElements::Rad(x)),
            TC::SPre(x) => Some(m::EG_OMathElements::SPre(x)),
            TC::SSub(x) => Some(m::EG_OMathElements::SSub(x)),
            TC::SSubSup(x) => Some(m::EG_OMathElements::SSubSup(x)),
            TC::SSup(x) => Some(m::EG_OMathElements::SSup(x)),
            TC::MR(x) => Some(m::EG_OMathElements::R(x)),
            other => {
                flush(&mut math, &mut out);
                out.push(match other {
                    TC::CustomXml(x) => EG_PContent::CustomXml(x),
                    TC::SmartTag(x) => EG_PContent::SmartTag(x),
                    TC::Sdt(x) => EG_PContent::Sdt(x),
                    TC::Dir(x) => EG_PContent::Dir(x),
                    TC::Bdo(x) => EG_PContent::Bdo(x),
                    TC::WR(x) => EG_PContent::R(x),
                    TC::ProofErr(x) => EG_PContent::ProofErr(x),
                    TC::PermStart(x) => EG_PContent::PermStart(x),
                    TC::PermEnd(x) => EG_PContent::PermEnd(x),
                    TC::BookmarkStart(x) => EG_PContent::BookmarkStart(x),
                    TC::BookmarkEnd(x) => EG_PContent::BookmarkEnd(x),
                    TC::MoveFromRangeStart(x) => EG_PContent::MoveFromRangeStart(x),
                    TC::MoveFromRangeEnd(x) => EG_PContent::MoveFromRangeEnd(x),
                    TC::MoveToRangeStart(x) => EG_PContent::MoveToRangeStart(x),
                    TC::MoveToRangeEnd(x) => EG_PContent::MoveToRangeEnd(x),
                    TC::CommentRangeStart(x) => EG_PContent::CommentRangeStart(x),
                    TC::CommentRangeEnd(x) => EG_PContent::CommentRangeEnd(x),
                    TC::CustomXmlInsRangeStart(x) => EG_PContent::CustomXmlInsRangeStart(x),
                    TC::CustomXmlInsRangeEnd(x) => EG_PContent::CustomXmlInsRangeEnd(x),
                    TC::CustomXmlDelRangeStart(x) => EG_PContent::CustomXmlDelRangeStart(x),
                    TC::CustomXmlDelRangeEnd(x) => EG_PContent::CustomXmlDelRangeEnd(x),
                    TC::CustomXmlMoveFromRangeStart(x) => EG_PContent::CustomXmlMoveFromRangeStart(x),
                    TC::CustomXmlMoveFromRangeEnd(x) => EG_PContent::CustomXmlMoveFromRangeEnd(x),
                    TC::CustomXmlMoveToRangeStart(x) => EG_PContent::CustomXmlMoveToRangeStart(x),
                    TC::CustomXmlMoveToRangeEnd(x) => EG_PContent::CustomXmlMoveToRangeEnd(x),
                    TC::Ins(x) => EG_PContent::Ins(x),
                    TC::Del(x) => EG_PContent::Del(x),
                    TC::MoveFrom(x) => EG_PContent::MoveFrom(x),
                    TC::MoveTo(x) => EG_PContent::MoveTo(x),
                    TC::OMathPara(x) => EG_PContent::OMathPara(x),
                    TC::OMath(x) => EG_PContent::OMath(x),
                    TC::Other(x) => EG_PContent::Other(x),
                    _ => unreachable!("math elements are handled above"),
                });
                None
            }
        };
        if let Some(e) = math_item {
            math.push(e);
        }
    }
    flush(&mut math, &mut out);
    out
}

/// Converts paragraph content into the content of a run-level change.
fn content_to_choice(item: EG_PContent) -> Option<TC> {
    Some(match item {
        EG_PContent::R(x) => TC::WR(x),
        EG_PContent::CustomXml(x) => TC::CustomXml(x),
        EG_PContent::SmartTag(x) => TC::SmartTag(x),
        EG_PContent::Sdt(x) => TC::Sdt(x),
        EG_PContent::ProofErr(x) => TC::ProofErr(x),
        EG_PContent::BookmarkStart(x) => TC::BookmarkStart(x),
        EG_PContent::BookmarkEnd(x) => TC::BookmarkEnd(x),
        EG_PContent::CommentRangeStart(x) => TC::CommentRangeStart(x),
        EG_PContent::CommentRangeEnd(x) => TC::CommentRangeEnd(x),
        _ => return None,
    })
}

/// Whether revisions are accepted or rejected.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Accept,
    Reject,
}

/// Visits revisions; `decide` selects those to process.
struct Pass<'a> {
    mode: Mode,
    decide: &'a mut dyn FnMut(&Revision) -> bool,
    count: usize,
    /// Whether move range markers are processed (not when listing).
    markers: bool,
    /// Ids of the move ranges removed so far (their end markers go too).
    moved: Vec<i64>,
}

impl Pass<'_> {
    fn select(
        &mut self,
        kind: RevisionKind,
        id: Option<i64>,
        author: &Option<String>,
        date: &Option<String>,
        text: String,
    ) -> bool {
        let rev = Revision {
            id: id.unwrap_or_default(),
            kind,
            author: author.clone(),
            date: date.clone(),
            text,
        };
        let yes = (self.decide)(&rev);
        if yes {
            self.count += 1;
        }
        yes
    }

    fn track(&mut self, kind: RevisionKind, t: &wml::CT_TrackChange, text: String) -> bool {
        self.select(kind, t.id, &t.author, &t.date, text)
    }

    fn run(&mut self, r: &mut wml::CT_R) {
        let text = revision_run_text(r);
        let Some(rpr) = r.r_pr.as_mut() else { return };
        let Some(change) = rpr.r_pr_change.as_ref() else {
            return;
        };
        if self.select(
            RevisionKind::RunFormatting,
            change.id,
            &change.author,
            &change.date,
            text,
        ) {
            let change = rpr.r_pr_change.take().expect("checked above");
            if self.mode == Mode::Reject {
                rpr.r_pr_base = change.r_pr.map(|o| o.r_pr_base).unwrap_or_default();
            }
        }
    }

    fn choices(&mut self, items: &mut Vec<TC>) {
        let mut i = 0;
        while i < items.len() {
            let (kind, keep_content) = match &items[i] {
                TC::Ins(_) => (RevisionKind::Insertion, self.mode == Mode::Accept),
                TC::MoveTo(_) => (RevisionKind::MoveTo, self.mode == Mode::Accept),
                TC::Del(_) => (RevisionKind::Deletion, self.mode == Mode::Reject),
                TC::MoveFrom(_) => (RevisionKind::MoveFrom, self.mode == Mode::Reject),
                TC::WR(_) => {
                    if let TC::WR(r) = &mut items[i] {
                        self.run(r);
                    }
                    i += 1;
                    continue;
                }
                _ => {
                    i += 1;
                    continue;
                }
            };
            let (TC::Ins(x) | TC::MoveTo(x) | TC::Del(x) | TC::MoveFrom(x)) = &mut items[i] else {
                unreachable!()
            };
            self.choices(&mut x.choice);
            let text = choices_text(&x.choice);
            if !self.select(kind, x.id, &x.author, &x.date, text) {
                i += 1;
                continue;
            }
            let (TC::Ins(x) | TC::MoveTo(x) | TC::Del(x) | TC::MoveFrom(x)) = items.remove(i) else {
                unreachable!()
            };
            if keep_content {
                let mut inner = x.choice;
                if matches!(kind, RevisionKind::Deletion | RevisionKind::MoveFrom) {
                    undelete_choices(&mut inner);
                }
                let n = inner.len();
                items.splice(i..i, inner);
                i += n;
            }
        }
    }

    fn content(&mut self, items: &mut Vec<EG_PContent>) {
        let mut i = 0;
        while i < items.len() {
            let (kind, keep_content) = match &mut items[i] {
                EG_PContent::Ins(_) => (RevisionKind::Insertion, self.mode == Mode::Accept),
                EG_PContent::MoveTo(_) => (RevisionKind::MoveTo, self.mode == Mode::Accept),
                EG_PContent::Del(_) => (RevisionKind::Deletion, self.mode == Mode::Reject),
                EG_PContent::MoveFrom(_) => (RevisionKind::MoveFrom, self.mode == Mode::Reject),
                EG_PContent::MoveFromRangeStart(b) | EG_PContent::MoveToRangeStart(b) => {
                    // Move ranges are not changes themselves: they go away
                    // with the moves they delimit (not listed).
                    let (id, author, date) = (b.id, b.author.clone(), b.date.clone());
                    let kind = if matches!(items[i], EG_PContent::MoveFromRangeStart(_)) {
                        RevisionKind::MoveFrom
                    } else {
                        RevisionKind::MoveTo
                    };
                    if self.markers
                        && (self.decide)(&Revision {
                            id: id.unwrap_or_default(),
                            kind,
                            author,
                            date,
                            text: String::new(),
                        })
                    {
                        self.moved.extend(id);
                        items.remove(i);
                    } else {
                        i += 1;
                    }
                    continue;
                }
                EG_PContent::MoveFromRangeEnd(e) | EG_PContent::MoveToRangeEnd(e) => {
                    if e.id.is_some_and(|id| self.moved.contains(&id)) {
                        items.remove(i);
                    } else {
                        i += 1;
                    }
                    continue;
                }
                EG_PContent::R(r) => {
                    self.run(r);
                    i += 1;
                    continue;
                }
                EG_PContent::Hyperlink(h) => {
                    self.content(&mut h.p_content);
                    i += 1;
                    continue;
                }
                EG_PContent::FldSimple(f) => {
                    self.content(&mut f.p_content);
                    i += 1;
                    continue;
                }
                EG_PContent::CustomXml(x) => {
                    self.content(&mut x.p_content);
                    i += 1;
                    continue;
                }
                EG_PContent::SmartTag(x) => {
                    self.content(&mut x.p_content);
                    i += 1;
                    continue;
                }
                EG_PContent::Dir(x) => {
                    self.content(&mut x.p_content);
                    i += 1;
                    continue;
                }
                EG_PContent::Bdo(x) => {
                    self.content(&mut x.p_content);
                    i += 1;
                    continue;
                }
                EG_PContent::Sdt(x) => {
                    if let Some(c) = x.sdt_content.as_mut() {
                        self.content(&mut c.p_content);
                    }
                    i += 1;
                    continue;
                }
                _ => {
                    i += 1;
                    continue;
                }
            };
            let (EG_PContent::Ins(x)
            | EG_PContent::MoveTo(x)
            | EG_PContent::Del(x)
            | EG_PContent::MoveFrom(x)) = &mut items[i]
            else {
                unreachable!()
            };
            self.choices(&mut x.choice);
            let text = choices_text(&x.choice);
            if !self.select(kind, x.id, &x.author, &x.date, text) {
                i += 1;
                continue;
            }
            let (EG_PContent::Ins(x)
            | EG_PContent::MoveTo(x)
            | EG_PContent::Del(x)
            | EG_PContent::MoveFrom(x)) = items.remove(i)
            else {
                unreachable!()
            };
            if keep_content {
                let mut inner = x.choice;
                if matches!(kind, RevisionKind::Deletion | RevisionKind::MoveFrom) {
                    undelete_choices(&mut inner);
                }
                let converted = choices_to_content(inner);
                let n = converted.len();
                items.splice(i..i, converted);
                i += n;
            }
        }
    }

    fn section(&mut self, sect: &mut wml::CT_SectPr) {
        let Some(change) = sect.sect_pr_change.as_ref() else {
            return;
        };
        if !self.select(
            RevisionKind::SectionFormatting,
            change.id,
            &change.author,
            &change.date,
            String::new(),
        ) {
            return;
        }
        let change = sect.sect_pr_change.take().expect("checked above");
        if self.mode == Mode::Reject
            && let Some(old) = change.sect_pr
            && let Ok(mut restored) = convert::<wml::CT_SectPrBase, wml::CT_SectPr>(&old, "sectPr")
        {
            restored.hdr_ftr_references = std::mem::take(&mut sect.hdr_ftr_references);
            *sect = restored;
        }
    }

    /// Processes a paragraph; returns true when its mark must be removed
    /// (the paragraph merges with the next one).
    fn paragraph(&mut self, p: &mut wml::CT_P) -> bool {
        self.content(&mut p.p_content);
        let text = text::paragraph_text(p);
        let Some(ppr) = p.p_pr.as_mut() else { return false };
        if let Some(sect) = ppr.sect_pr.as_mut() {
            self.section(sect);
        }
        if let Some(change) = ppr.p_pr_change.as_ref()
            && self.select(
                RevisionKind::ParagraphFormatting,
                change.id,
                &change.author,
                &change.date,
                String::new(),
            )
        {
            let change = ppr.p_pr_change.take().expect("checked above");
            if self.mode == Mode::Reject
                && let Some(old) = change.p_pr
                && let Ok(mut restored) = convert::<wml::CT_PPrBase, wml::CT_PPr>(&old, "pPr")
            {
                restored.r_pr = ppr.r_pr.take();
                restored.sect_pr = ppr.sect_pr.take();
                **ppr = restored;
            }
        }
        let mut merge = false;
        if let Some(mark) = ppr.r_pr.as_mut() {
            if let Some(change) = mark.r_pr_change.as_ref()
                && self.select(
                    RevisionKind::RunFormatting,
                    change.id,
                    &change.author,
                    &change.date,
                    String::new(),
                )
            {
                let change = mark.r_pr_change.take().expect("checked above");
                if self.mode == Mode::Reject {
                    mark.r_pr_base = change.r_pr.map(|o| o.r_pr_base).unwrap_or_default();
                }
            }
            if let Some(ins) = mark.ins.as_deref()
                && self.track(RevisionKind::ParagraphInsertion, ins, text.clone())
            {
                mark.ins = None;
                merge |= self.mode == Mode::Reject;
            }
            if let Some(del) = mark.del.as_deref()
                && self.track(RevisionKind::ParagraphDeletion, del, text.clone())
            {
                mark.del = None;
                merge |= self.mode == Mode::Accept;
            }
            for (slot, kind) in [
                (&mut mark.move_from, RevisionKind::MoveFrom),
                (&mut mark.move_to, RevisionKind::MoveTo),
            ] {
                if let Some(t) = slot.as_deref()
                    && self.track(kind, t, String::new())
                {
                    *slot = None;
                }
            }
        }
        merge
    }

    fn table(&mut self, t: &mut wml::CT_Tbl) {
        if let Some(tpr) = t.tbl_pr.as_mut()
            && let Some(change) = tpr.tbl_pr_change.as_ref()
            && self.select(
                RevisionKind::TableFormatting,
                change.id,
                &change.author,
                &change.date,
                String::new(),
            )
        {
            let change = tpr.tbl_pr_change.take().expect("checked above");
            if self.mode == Mode::Reject
                && let Some(old) = change.tbl_pr
                && let Ok(restored) = convert::<wml::CT_TblPrBase, wml::CT_TblPr>(&old, "tblPr")
            {
                **tpr = restored;
            }
        }
        if let Some(grid) = t.tbl_grid.as_mut()
            && let Some(change) = grid.tbl_grid_change.as_ref()
            && self.select(
                RevisionKind::TableFormatting,
                change.id,
                &None,
                &None,
                String::new(),
            )
        {
            let change = grid.tbl_grid_change.take().expect("checked above");
            if self.mode == Mode::Reject
                && let Some(old) = change.tbl_grid
            {
                grid.grid_col = old.grid_col;
            }
        }
        self.rows(&mut t.content_row_content);
    }

    fn rows(&mut self, items: &mut Vec<EG_ContentRowContent>) {
        let mut i = 0;
        while i < items.len() {
            let mut remove = false;
            match &mut items[i] {
                EG_ContentRowContent::Tr(row) => remove = self.row(row),
                EG_ContentRowContent::Sdt(s) => {
                    if let Some(c) = s.sdt_content.as_mut() {
                        self.rows(&mut c.content_row_content);
                    }
                }
                EG_ContentRowContent::CustomXml(x) => self.rows(&mut x.content_row_content),
                _ => {}
            }
            if remove {
                items.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Returns true when the row must be removed.
    fn row(&mut self, row: &mut wml::CT_Row) -> bool {
        let text = text::table_text(&wml::CT_Tbl {
            content_row_content: vec![EG_ContentRowContent::Tr(Box::new(row.clone()))],
            ..Default::default()
        });
        let mut remove = false;
        if let Some(pr_ex) = row.tbl_pr_ex.as_mut()
            && let Some(change) = pr_ex.tbl_pr_ex_change.as_ref()
            && self.select(
                RevisionKind::TableFormatting,
                change.id,
                &change.author,
                &change.date,
                String::new(),
            )
        {
            let change = pr_ex.tbl_pr_ex_change.take().expect("checked above");
            if self.mode == Mode::Reject
                && let Some(old) = change.tbl_pr_ex
                && let Ok(restored) = convert::<wml::CT_TblPrExBase, wml::CT_TblPrEx>(&old, "tblPrEx")
            {
                **pr_ex = restored;
            }
        }
        if let Some(tr_pr) = row.tr_pr.as_mut() {
            if let Some(change) = tr_pr.tr_pr_change.as_ref()
                && self.select(
                    RevisionKind::TableFormatting,
                    change.id,
                    &change.author,
                    &change.date,
                    String::new(),
                )
            {
                let change = tr_pr.tr_pr_change.take().expect("checked above");
                if self.mode == Mode::Reject
                    && let Some(old) = change.tr_pr
                    && let Ok(restored) = convert::<wml::CT_TrPrBase, wml::CT_TrPr>(&old, "trPr")
                {
                    tr_pr.choice = restored.choice;
                }
            }
            if let Some(ins) = tr_pr.ins.as_deref()
                && self.track(RevisionKind::RowInsertion, ins, text.clone())
            {
                tr_pr.ins = None;
                remove |= self.mode == Mode::Reject;
            }
            if let Some(del) = tr_pr.del.as_deref()
                && self.track(RevisionKind::RowDeletion, del, text.clone())
            {
                tr_pr.del = None;
                remove |= self.mode == Mode::Accept;
            }
        }
        for cell in text::cells_mut(row) {
            self.cell(cell);
        }
        remove
    }

    fn cell(&mut self, cell: &mut wml::CT_Tc) {
        if let Some(tc_pr) = cell.tc_pr.as_mut() {
            if let Some(change) = tc_pr.tc_pr_change.as_ref()
                && self.select(
                    RevisionKind::TableFormatting,
                    change.id,
                    &change.author,
                    &change.date,
                    String::new(),
                )
            {
                let change = tc_pr.tc_pr_change.take().expect("checked above");
                if self.mode == Mode::Reject
                    && let Some(old) = change.tc_pr
                    && let Ok(restored) = convert::<wml::CT_TcPrInner, wml::CT_TcPr>(&old, "tcPr")
                {
                    **tc_pr = restored;
                }
            }
            let marker = match &tc_pr.cell_markup_elements {
                Some(EG_CellMarkupElements::CellIns(t) | EG_CellMarkupElements::CellDel(t)) => {
                    Some((t.id, t.author.clone(), t.date.clone()))
                }
                Some(EG_CellMarkupElements::CellMerge(t)) => Some((t.id, t.author.clone(), t.date.clone())),
                _ => None,
            };
            // Cell insertions, deletions and merges are only unmarked.
            if let Some((id, author, date)) = marker
                && self.select(RevisionKind::TableFormatting, id, &author, &date, String::new())
            {
                tc_pr.cell_markup_elements = None;
            }
        }
        self.blocks(&mut cell.block_level_elts);
    }

    fn blocks(&mut self, items: &mut Vec<EG_BlockLevelElts>) {
        let mut i = 0;
        while i < items.len() {
            let mut merge = match &mut items[i] {
                EG_BlockLevelElts::P(p) => self.paragraph(p),
                EG_BlockLevelElts::Tbl(t) => {
                    self.table(t);
                    false
                }
                EG_BlockLevelElts::Sdt(s) => {
                    if let Some(c) = s.sdt_content.as_mut() {
                        self.content_blocks(&mut c.content_block_content);
                    }
                    false
                }
                EG_BlockLevelElts::CustomXml(x) => {
                    self.content_blocks(&mut x.content_block_content);
                    false
                }
                _ => false,
            };
            // A removed paragraph mark joins the paragraph with the next one,
            // which is processed first (its own mark may be removed too).
            while merge {
                let next_merge = match items.get_mut(i + 1) {
                    Some(EG_BlockLevelElts::P(next)) => self.paragraph(next),
                    _ => break,
                };
                let EG_BlockLevelElts::P(next) = items.remove(i + 1) else {
                    unreachable!("checked above")
                };
                let EG_BlockLevelElts::P(p) = &mut items[i] else {
                    unreachable!("merging starts from a paragraph")
                };
                // The merged paragraph keeps the surviving (second) paragraph mark.
                let mut next = *next;
                p.p_content.append(&mut next.p_content);
                p.p_pr = next.p_pr;
                merge = next_merge;
            }
            i += 1;
        }
    }

    fn content_blocks(&mut self, items: &mut [EG_ContentBlockContent]) {
        for item in items {
            match item {
                EG_ContentBlockContent::P(p) => {
                    // Paragraph marks inside content controls are unmarked without merging.
                    self.paragraph(p);
                }
                EG_ContentBlockContent::Tbl(t) => self.table(t),
                EG_ContentBlockContent::Sdt(s) => {
                    if let Some(c) = s.sdt_content.as_mut() {
                        self.content_blocks(&mut c.content_block_content);
                    }
                }
                EG_ContentBlockContent::CustomXml(x) => self.content_blocks(&mut x.content_block_content),
                _ => {}
            }
        }
    }
}

impl ParagraphMut<'_> {
    /// Appends `text` as a tracked insertion (`w:ins`). Returns the revision id.
    pub fn add_insertion(&mut self, text: &str, info: &RevisionInfo) -> i64 {
        let id = self.shared.new_id();
        let mut r = wml::CT_R::default();
        RunMut::new(&mut r).add_text(text);
        self.p
            .p_content
            .push(EG_PContent::Ins(Box::new(wml::CT_RunTrackChange {
                id: Some(id),
                author: Some(info.author.clone()),
                date: Some(info.date()),
                choice: vec![TC::WR(Box::new(r))],
                ..Default::default()
            })));
        id
    }

    /// Appends `text` as a tracked deletion (`w:del` with `w:delText`):
    /// text that was removed while changes were tracked. Returns the revision id.
    pub fn add_deletion(&mut self, text: &str, info: &RevisionInfo) -> i64 {
        let id = self.shared.new_id();
        let r = wml::CT_R {
            run_inner_content: vec![EG_RunInnerContent::DelText(text_node(text))],
            ..Default::default()
        };
        self.p
            .p_content
            .push(EG_PContent::Del(Box::new(wml::CT_RunTrackChange {
                id: Some(id),
                author: Some(info.author.clone()),
                date: Some(info.date()),
                choice: vec![TC::WR(Box::new(r))],
                ..Default::default()
            })));
        id
    }

    /// Marks existing content of the paragraph as deleted (a tracked
    /// deletion). The span may contain runs, bookmarks and comment ranges
    /// only. Returns the revision id.
    pub fn track_deletion(&mut self, span: TextSpan<'_>, info: &RevisionInfo) -> Result<i64> {
        let range = markup::resolve_span(self.p, span)?;
        if range.is_empty() {
            return Err(Error::InvalidArgument("nothing to delete".into()));
        }
        let movable = self.p.p_content[range.clone()]
            .iter()
            .all(|c| content_to_choice(c.clone()).is_some());
        if !movable {
            return Err(Error::InvalidArgument(
                "only runs can be marked as deleted (the span contains hyperlinks, fields or changes)".into(),
            ));
        }
        let id = self.shared.new_id();
        let removed: Vec<EG_PContent> = self.p.p_content.drain(range.clone()).collect();
        let mut choice: Vec<TC> = removed.into_iter().filter_map(content_to_choice).collect();
        for c in &mut choice {
            if let TC::WR(r) = c {
                delete_run(r);
            }
        }
        self.p.p_content.insert(
            range.start,
            EG_PContent::Del(Box::new(wml::CT_RunTrackChange {
                id: Some(id),
                author: Some(info.author.clone()),
                date: Some(info.date()),
                choice,
                ..Default::default()
            })),
        );
        Ok(id)
    }

    /// Changes the formatting of the `run`-th visible run (see
    /// [`crate::Paragraph::runs`]) as a tracked change: the current run
    /// properties are recorded in `w:rPrChange` before `edit` applies the
    /// new formatting. Returns the revision id.
    pub fn track_format_change(
        &mut self,
        run: usize,
        info: &RevisionInfo,
        edit: impl FnOnce(&mut RunMut<'_>),
    ) -> Result<i64> {
        let id = self.shared.new_id();
        let mut runs = text::runs_mut(&mut self.p.p_content);
        let count = runs.len();
        let r = runs
            .get_mut(run)
            .ok_or_else(|| Error::NotFound(format!("run {run} (the paragraph has {count})")))?;
        let rpr = r.r_pr.get_or_insert_with(Default::default);
        let original = wml::CT_RPrOriginal {
            r_pr_base: rpr.r_pr_base.clone(),
            ..Default::default()
        };
        rpr.r_pr_change = Some(Box::new(wml::CT_RPrChange {
            id: Some(id),
            author: Some(info.author.clone()),
            date: Some(info.date()),
            r_pr: Some(Box::new(original)),
            ..Default::default()
        }));
        edit(&mut RunMut::new(r));
        Ok(id)
    }
}

impl Document {
    /// Turns revision tracking on or off (`w:trackRevisions` in the settings).
    pub fn set_track_revisions(&mut self, value: bool) -> Result<()> {
        self.shared.settings_mut()?.track_revisions = value.then(on);
        Ok(())
    }

    /// Whether revision tracking is on.
    pub fn track_revisions(&self) -> bool {
        self.settings().is_some_and(|s| is_on(&s.track_revisions))
    }

    /// Runs a pass over every story (body, headers and footers, notes and
    /// comments), marking the changed parts as modified.
    fn revision_pass(&mut self, mode: Mode, decide: &mut dyn FnMut(&Revision) -> bool) -> usize {
        let mut pass = Pass {
            mode,
            decide,
            count: 0,
            markers: true,
            moved: Vec::new(),
        };
        if let Some(body) = self.main.body.as_mut() {
            let before = pass.count;
            pass.blocks(&mut body.block_level_elts);
            if let Some(sect) = body.sect_pr.as_mut() {
                pass.section(sect);
            }
            self.main_dirty |= pass.count != before;
        }
        for h in &mut self.headers {
            let before = pass.count;
            pass.blocks(&mut h.part.value.block_level_elts);
            h.part.dirty |= pass.count != before;
        }
        if let Some(t) = self.shared.footnotes.as_mut() {
            let before = pass.count;
            for n in &mut t.value.footnote {
                pass.blocks(&mut n.block_level_elts);
            }
            t.dirty |= pass.count != before;
        }
        if let Some(t) = self.shared.endnotes.as_mut() {
            let before = pass.count;
            for n in &mut t.value.endnote {
                pass.blocks(&mut n.block_level_elts);
            }
            t.dirty |= pass.count != before;
        }
        if let Some(t) = self.shared.comments.as_mut() {
            let before = pass.count;
            for c in &mut t.value.comment {
                pass.blocks(&mut c.block_level_elts);
            }
            t.dirty |= pass.count != before;
        }
        pass.count
    }

    /// Tracked changes of every story (body, headers and footers, notes,
    /// comments) in document order.
    pub fn revisions(&self) -> Vec<Revision> {
        let mut out = Vec::new();
        let mut record = |r: &Revision| {
            out.push(r.clone());
            false
        };
        let mut pass = Pass {
            mode: Mode::Accept,
            decide: &mut record,
            count: 0,
            markers: false,
            moved: Vec::new(),
        };
        // The pass edits what it visits, so it works on copies.
        let mut body = self.body().clone();
        pass.blocks(&mut body.block_level_elts);
        if let Some(sect) = body.sect_pr.as_mut() {
            pass.section(sect);
        }
        for h in &self.headers {
            pass.blocks(&mut h.part.value.block_level_elts.clone());
        }
        if let Some(t) = &self.shared.footnotes {
            for n in &t.value.footnote {
                pass.blocks(&mut n.block_level_elts.clone());
            }
        }
        if let Some(t) = &self.shared.endnotes {
            for n in &t.value.endnote {
                pass.blocks(&mut n.block_level_elts.clone());
            }
        }
        if let Some(t) = &self.shared.comments {
            for c in &t.value.comment {
                pass.blocks(&mut c.block_level_elts.clone());
            }
        }
        out
    }

    /// Accepts every tracked change. Returns the number of changes accepted.
    pub fn accept_all_revisions(&mut self) -> usize {
        self.revision_pass(Mode::Accept, &mut |_| true)
    }

    /// Rejects every tracked change. Returns the number of changes rejected.
    pub fn reject_all_revisions(&mut self) -> usize {
        self.revision_pass(Mode::Reject, &mut |_| true)
    }

    /// Accepts the tracked changes made by `author`.
    pub fn accept_revisions_by(&mut self, author: &str) -> usize {
        self.revision_pass(Mode::Accept, &mut |r| r.author.as_deref() == Some(author))
    }

    /// Rejects the tracked changes made by `author`.
    pub fn reject_revisions_by(&mut self, author: &str) -> usize {
        self.revision_pass(Mode::Reject, &mut |r| r.author.as_deref() == Some(author))
    }

    /// Accepts the tracked changes selected by `filter`.
    pub fn accept_revisions_where(&mut self, mut filter: impl FnMut(&Revision) -> bool) -> usize {
        self.revision_pass(Mode::Accept, &mut filter)
    }

    /// Rejects the tracked changes selected by `filter`.
    pub fn reject_revisions_where(&mut self, mut filter: impl FnMut(&Revision) -> bool) -> usize {
        self.revision_pass(Mode::Reject, &mut filter)
    }
}

/// Parses body content for tests.
#[cfg(test)]
fn parse_body(xml: &str) -> wml::CT_Body {
    let doc = wml::elements::DOCUMENT
        .parse(&format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><w:body>{xml}</w:body></w:document>"#
        ))
        .unwrap();
    *doc.body.unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk;

    fn run_pass(xml: &str, mode: Mode) -> (wml::CT_Body, usize) {
        let mut body = parse_body(xml);
        let mut all = |_: &Revision| true;
        let mut pass = Pass {
            mode,
            decide: &mut all,
            count: 0,
            markers: true,
            moved: Vec::new(),
        };
        pass.blocks(&mut body.block_level_elts);
        let n = pass.count;
        (body, n)
    }

    fn body_text(body: &wml::CT_Body) -> String {
        text::blocks_text(&text::blocks(&body.block_level_elts))
    }

    const MIXED: &str = r#"<w:p><w:r><w:t xml:space="preserve">a </w:t></w:r><w:ins w:id="1" w:author="x"><w:r><w:t>new</w:t></w:r></w:ins><w:del w:id="2" w:author="y"><w:r><w:delText>old</w:delText></w:r></w:del><w:r><w:rPr><w:b/><w:rPrChange w:id="3" w:author="x"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t xml:space="preserve"> z</w:t></w:r></w:p>"#;

    #[test]
    fn accept_and_reject_run_level_changes() {
        let (accepted, n) = run_pass(MIXED, Mode::Accept);
        assert_eq!(n, 3);
        assert_eq!(body_text(&accepted), "a new z");
        let (rejected, n) = run_pass(MIXED, Mode::Reject);
        assert_eq!(n, 3);
        assert_eq!(body_text(&rejected), "a old z");
        let EG_BlockLevelElts::P(p) = &rejected.block_level_elts[0] else {
            panic!()
        };
        let runs = text::runs(&p.p_content);
        let last = runs.last().unwrap();
        assert!(crate::Run::new(last).is_italic());
        assert!(!crate::Run::new(last).is_bold());
        assert!(last.r_pr.as_ref().unwrap().r_pr_change.is_none());
    }

    #[test]
    fn paragraph_marks_merge_paragraphs() {
        let xml = r#"<w:p><w:pPr><w:jc w:val="center"/><w:rPr><w:del w:id="1" w:author="x"/></w:rPr></w:pPr><w:r><w:t>one </w:t></w:r></w:p><w:p><w:pPr><w:jc w:val="right"/></w:pPr><w:r><w:t>two</w:t></w:r></w:p>"#;
        let (accepted, n) = run_pass(xml, Mode::Accept);
        assert_eq!(n, 1);
        assert_eq!(accepted.block_level_elts.len(), 1);
        assert_eq!(body_text(&accepted), "one two");
        let EG_BlockLevelElts::P(p) = &accepted.block_level_elts[0] else {
            panic!()
        };
        assert_eq!(
            crate::Paragraph::new(p).alignment(),
            Some(crate::Alignment::Right)
        );
        let (rejected, _) = run_pass(xml, Mode::Reject);
        assert_eq!(rejected.block_level_elts.len(), 2);

        let inserted = xml.replace("w:del ", "w:ins ");
        let (rejected, _) = run_pass(&inserted, Mode::Reject);
        assert_eq!(rejected.block_level_elts.len(), 1);
        let (accepted, _) = run_pass(&inserted, Mode::Accept);
        assert_eq!(accepted.block_level_elts.len(), 2);
    }

    #[test]
    fn nested_changes_moves_and_rows() {
        let xml = r#"<w:p><w:moveFromRangeStart w:id="5" w:name="m" w:author="x"/><w:moveFrom w:id="6" w:author="x"><w:r><w:t>moved</w:t></w:r></w:moveFrom><w:moveFromRangeEnd w:id="5"/><w:ins w:id="7" w:author="x"><w:del w:id="8" w:author="y"><w:r><w:delText>gone</w:delText></w:r></w:del><w:r><w:t>kept</w:t></w:r></w:ins></w:p><w:tbl><w:tr><w:trPr><w:del w:id="9" w:author="x"/></w:trPr><w:tc><w:p><w:r><w:t>row</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>"#;
        let (accepted, _) = run_pass(xml, Mode::Accept);
        assert_eq!(body_text(&accepted), "kept\n");
        let EG_BlockLevelElts::P(p) = &accepted.block_level_elts[0] else {
            panic!()
        };
        assert_eq!(p.p_content.len(), 1);
        let (rejected, _) = run_pass(xml, Mode::Reject);
        assert_eq!(body_text(&rejected), "moved\nrow\n");
        let EG_BlockLevelElts::P(p) = &rejected.block_level_elts[0] else {
            panic!()
        };
        // The move range markers went with the move.
        assert!(!p.p_content.iter().any(|c| matches!(
            c,
            EG_PContent::MoveFromRangeStart(_) | EG_PContent::MoveFromRangeEnd(_)
        )));

        // Listing reports the changes, not the range markers.
        let mut doc = Document::new();
        *doc.body_mut() = parse_body(xml);
        let revs = doc.revisions();
        let kinds: Vec<RevisionKind> = revs.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RevisionKind::MoveFrom,
                RevisionKind::Deletion,
                RevisionKind::Insertion,
                RevisionKind::RowDeletion
            ]
        );
        assert_eq!(revs[2].text, "gonekept");
        assert_eq!(doc.accept_revisions_by("y"), 1);
        assert_eq!(doc.revisions().len(), 3);
    }

    #[test]
    fn math_inside_insertions_is_rewrapped() {
        let items = vec![
            TC::MR(Box::default()),
            TC::F(Box::default()),
            TC::WR(Box::default()),
            TC::MR(Box::default()),
        ];
        let out = choices_to_content(items);
        assert_eq!(out.len(), 3);
        assert!(matches!(&out[0], EG_PContent::OMath(m) if m.o_math_elements.len() == 2));
        assert!(matches!(out[1], EG_PContent::R(_)));
        assert!(matches!(out[2], EG_PContent::OMath(_)));
    }

    #[test]
    fn walker_helpers_are_consistent() {
        let body = parse_body(MIXED);
        let mut n = 0;
        walk::walk_blocks_ref(&body.block_level_elts, &mut |_| n += 1);
        assert_eq!(n, 1);
        let EG_BlockLevelElts::P(p) = &body.block_level_elts[0] else {
            panic!()
        };
        assert_eq!(content_text(&p.p_content), "a newold z");
    }
}
