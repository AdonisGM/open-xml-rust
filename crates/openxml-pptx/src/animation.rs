//! Animations, in the structure PowerPoint writes: a `tmRoot` parallel node
//! holding the main sequence (`mainSeq`) of click groups, each holding
//! timed groups of effects, plus build entries (`p:bldLst`) for shapes.

use openxml_core::{Error, Result};
use openxml_schema::pml;
use openxml_xml::{Ns, RawElement};

use crate::slide::{Slide, SlideMut};
use crate::util;

/// The direction an effect comes from (entrance) or goes to (exit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// The bottom of the slide.
    Bottom,
    /// The left of the slide.
    Left,
    /// The right of the slide.
    Right,
    /// The top of the slide.
    Top,
}

impl Direction {
    fn subtype(self) -> i32 {
        match self {
            Direction::Top => 1,
            Direction::Right => 2,
            Direction::Bottom => 4,
            Direction::Left => 8,
        }
    }

    fn from_subtype(s: i32) -> Option<Direction> {
        Some(match s {
            1 => Direction::Top,
            2 => Direction::Right,
            4 => Direction::Bottom,
            8 => Direction::Left,
            _ => return None,
        })
    }

    /// Off-slide position `(attribute, value)` for fly effects.
    fn off_slide(self) -> (&'static str, &'static str) {
        match self {
            Direction::Bottom => ("ppt_y", "1+#ppt_h/2"),
            Direction::Top => ("ppt_y", "0-#ppt_h/2"),
            Direction::Left => ("ppt_x", "0-#ppt_w/2"),
            Direction::Right => ("ppt_x", "1+#ppt_w/2"),
        }
    }

    fn wipe_in(self) -> &'static str {
        match self {
            Direction::Bottom => "wipe(up)",
            Direction::Top => "wipe(down)",
            Direction::Left => "wipe(right)",
            Direction::Right => "wipe(left)",
        }
    }

    fn wipe_out(self) -> &'static str {
        match self {
            Direction::Bottom => "wipe(down)",
            Direction::Top => "wipe(up)",
            Direction::Left => "wipe(left)",
            Direction::Right => "wipe(right)",
        }
    }
}

/// An animation effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Entrance: the shape appears at once.
    Appear,
    /// Entrance: the shape fades in.
    FadeIn,
    /// Entrance: the shape flies in from a side of the slide.
    FlyIn(Direction),
    /// Entrance: the shape is revealed from one side.
    WipeIn(Direction),
    /// Exit: the shape disappears at once.
    Disappear,
    /// Exit: the shape fades out.
    FadeOut,
    /// Exit: the shape flies out towards a side of the slide.
    FlyOut(Direction),
    /// Exit: the shape is wiped away towards one side.
    WipeOut(Direction),
    /// Emphasis: the shape spins by the given angle in degrees.
    Spin(f64),
    /// Emphasis: the shape grows (> 1.0) or shrinks (< 1.0) by a scale factor.
    GrowShrink(f64),
}

/// The class of an effect (`presetClass`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectClass {
    /// Entrance.
    Entrance,
    /// Exit.
    Exit,
    /// Emphasis.
    Emphasis,
    /// Motion path.
    MotionPath,
    /// Another class (OLE verbs, media calls).
    Other,
}

impl Effect {
    /// The class of the effect.
    pub fn class(self) -> EffectClass {
        match self {
            Effect::Appear | Effect::FadeIn | Effect::FlyIn(_) | Effect::WipeIn(_) => EffectClass::Entrance,
            Effect::Disappear | Effect::FadeOut | Effect::FlyOut(_) | Effect::WipeOut(_) => EffectClass::Exit,
            Effect::Spin(_) | Effect::GrowShrink(_) => EffectClass::Emphasis,
        }
    }

    /// `(presetID, presetSubtype)` of PowerPoint's preset.
    fn preset(self) -> (i32, i32) {
        match self {
            Effect::Appear | Effect::Disappear => (1, 0),
            Effect::FlyIn(d) | Effect::FlyOut(d) => (2, d.subtype()),
            Effect::GrowShrink(_) => (6, 0),
            Effect::Spin(_) => (8, 0),
            Effect::FadeIn | Effect::FadeOut => (10, 0),
            Effect::WipeIn(d) | Effect::WipeOut(d) => (22, d.subtype()),
        }
    }

    fn preset_class(self) -> &'static str {
        match self.class() {
            EffectClass::Entrance => "entr",
            EffectClass::Exit => "exit",
            _ => "emph",
        }
    }

    /// Whether the effect is instantaneous (its duration is ignored).
    fn is_instant(self) -> bool {
        matches!(self, Effect::Appear | Effect::Disappear)
    }
}

/// What starts an animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// A mouse click (or the next-slide key).
    OnClick,
    /// Together with the previous animation.
    WithPrevious,
    /// When the previous animation ends.
    AfterPrevious,
}

impl Trigger {
    fn node_type(self) -> &'static str {
        match self {
            Trigger::OnClick => "clickEffect",
            Trigger::WithPrevious => "withEffect",
            Trigger::AfterPrevious => "afterEffect",
        }
    }
}

/// An animation to add to a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Animation {
    /// The effect.
    pub effect: Effect,
    /// What starts it.
    pub trigger: Trigger,
    /// Delay after the trigger, in milliseconds.
    pub delay_ms: u32,
    /// Duration in milliseconds (ignored by instantaneous effects).
    pub duration_ms: u32,
}

impl Animation {
    /// An effect started by a click, without delay, with PowerPoint's default
    /// duration (0.5 s; 2 s for emphasis effects).
    pub fn new(effect: Effect) -> Animation {
        Animation {
            effect,
            trigger: Trigger::OnClick,
            delay_ms: 0,
            duration_ms: if effect.class() == EffectClass::Emphasis {
                2000
            } else {
                500
            },
        }
    }

    /// Sets the trigger.
    pub fn trigger(mut self, trigger: Trigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// Sets the delay in milliseconds.
    pub fn delay(mut self, ms: u32) -> Self {
        self.delay_ms = ms;
        self
    }

    /// Sets the duration in milliseconds.
    pub fn duration(mut self, ms: u32) -> Self {
        self.duration_ms = ms;
        self
    }
}

/// An animation found on a slide's main sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationInfo {
    /// The animated shape.
    pub shape_id: u32,
    /// Class of the effect.
    pub class: EffectClass,
    /// PowerPoint preset identifier (`presetID`).
    pub preset_id: i32,
    /// The effect, when it is one this crate creates.
    pub effect: Option<Effect>,
    /// What starts it.
    pub trigger: Trigger,
    /// Delay after the trigger, in milliseconds.
    pub delay_ms: u32,
    /// Duration in milliseconds (0 for instantaneous effects).
    pub duration_ms: u32,
}

/// Allocates time node identifiers.
pub(crate) struct Ids(u32);

impl Ids {
    pub(crate) fn next(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }
}

fn target(spid: u32) -> String {
    format!(r#"<p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl>"#)
}

fn set_visibility(ids: &mut Ids, spid: u32, value: &str, delay: u32) -> String {
    format!(
        concat!(
            r#"<p:set><p:cBhvr><p:cTn id="{id}" dur="1" fill="hold"><p:stCondLst><p:cond delay="{delay}"/></p:stCondLst></p:cTn>"#,
            r#"{tgt}<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr>"#,
            r#"<p:to><p:strVal val="{value}"/></p:to></p:set>"#
        ),
        id = ids.next(),
        delay = delay,
        tgt = target(spid),
        value = value
    )
}

fn anim_effect(ids: &mut Ids, spid: u32, transition: &str, filter: &str, dur: u32) -> String {
    format!(
        r#"<p:animEffect transition="{transition}" filter="{filter}"><p:cBhvr><p:cTn id="{id}" dur="{dur}"/>{tgt}</p:cBhvr></p:animEffect>"#,
        id = ids.next(),
        tgt = target(spid)
    )
}

fn anim_property(ids: &mut Ids, spid: u32, attr: &str, from: &str, to: &str, dur: u32) -> String {
    format!(
        concat!(
            r#"<p:anim calcmode="lin" valueType="num"><p:cBhvr additive="base"><p:cTn id="{id}" dur="{dur}" fill="hold"/>"#,
            r#"{tgt}<p:attrNameLst><p:attrName>{attr}</p:attrName></p:attrNameLst></p:cBhvr>"#,
            r#"<p:tavLst><p:tav tm="0"><p:val><p:strVal val="{from}"/></p:val></p:tav>"#,
            r#"<p:tav tm="100000"><p:val><p:strVal val="{to}"/></p:val></p:tav></p:tavLst></p:anim>"#
        ),
        id = ids.next(),
        dur = dur,
        tgt = target(spid),
        attr = attr,
        from = from,
        to = to
    )
}

fn behaviours(ids: &mut Ids, spid: u32, effect: Effect, dur: u32) -> String {
    let dur = dur.max(1);
    match effect {
        Effect::Appear => set_visibility(ids, spid, "visible", 0),
        Effect::Disappear => set_visibility(ids, spid, "hidden", 0),
        Effect::FadeIn => {
            set_visibility(ids, spid, "visible", 0) + &anim_effect(ids, spid, "in", "fade", dur)
        }
        Effect::WipeIn(d) => {
            set_visibility(ids, spid, "visible", 0) + &anim_effect(ids, spid, "in", d.wipe_in(), dur)
        }
        Effect::FlyIn(d) => {
            let (attr, from) = d.off_slide();
            let start = |a: &'static str| {
                if a == attr {
                    from
                } else if a == "ppt_x" {
                    "#ppt_x"
                } else {
                    "#ppt_y"
                }
            };
            set_visibility(ids, spid, "visible", 0)
                + &anim_property(ids, spid, "ppt_x", start("ppt_x"), "#ppt_x", dur)
                + &anim_property(ids, spid, "ppt_y", start("ppt_y"), "#ppt_y", dur)
        }
        Effect::FadeOut => {
            anim_effect(ids, spid, "out", "fade", dur) + &set_visibility(ids, spid, "hidden", dur - 1)
        }
        Effect::WipeOut(d) => {
            anim_effect(ids, spid, "out", d.wipe_out(), dur) + &set_visibility(ids, spid, "hidden", dur - 1)
        }
        Effect::FlyOut(d) => {
            let (attr, to) = d.off_slide();
            let end = |a: &'static str| {
                if a == attr {
                    to
                } else if a == "ppt_x" {
                    "#ppt_x"
                } else {
                    "#ppt_y"
                }
            };
            anim_property(ids, spid, "ppt_x", "#ppt_x", end("ppt_x"), dur)
                + &anim_property(ids, spid, "ppt_y", "#ppt_y", end("ppt_y"), dur)
                + &set_visibility(ids, spid, "hidden", dur - 1)
        }
        Effect::Spin(degrees) => format!(
            concat!(
                r#"<p:animRot by="{by}"><p:cBhvr><p:cTn id="{id}" dur="{dur}" fill="hold"/>{tgt}"#,
                r#"<p:attrNameLst><p:attrName>r</p:attrName></p:attrNameLst></p:cBhvr></p:animRot>"#
            ),
            by = (degrees * 60_000.0).round() as i64,
            id = ids.next(),
            dur = dur,
            tgt = target(spid)
        ),
        Effect::GrowShrink(scale) => format!(
            r#"<p:animScale><p:cBhvr><p:cTn id="{id}" dur="{dur}" fill="hold"/>{tgt}</p:cBhvr><p:by x="{s}" y="{s}"/></p:animScale>"#,
            id = ids.next(),
            dur = dur,
            tgt = target(spid),
            s = (scale * 100_000.0).round() as i64
        ),
    }
}

/// The effect node (`p:par` with a preset `cTn`) of an animation.
fn effect_par(ids: &mut Ids, spid: u32, grp_id: u32, a: &Animation) -> pml::CT_TLTimeNodeParallel {
    let (preset, subtype) = a.effect.preset();
    let id = ids.next();
    let body = behaviours(ids, spid, a.effect, a.duration_ms);
    util::fragment(&format!(
        concat!(
            r#"<p:par><p:cTn id="{id}" presetID="{preset}" presetClass="{class}" presetSubtype="{subtype}" fill="hold" "#,
            r#"grpId="{grp}" nodeType="{node}"><p:stCondLst><p:cond delay="{delay}"/></p:stCondLst>"#,
            r#"<p:childTnLst>{body}</p:childTnLst></p:cTn></p:par>"#
        ),
        id = id,
        preset = preset,
        class = a.effect.preset_class(),
        subtype = subtype,
        grp = grp_id,
        node = a.trigger.node_type(),
        delay = a.delay_ms,
        body = body
    ))
}

/// A `p:par` grouping node started after `delay` (`indefinite` = on click).
fn group_par(ids: &mut Ids, delay: &str, extra_cond: &str) -> pml::CT_TLTimeNodeParallel {
    util::fragment(&format!(
        r#"<p:par><p:cTn id="{id}" fill="hold"><p:stCondLst><p:cond delay="{delay}"/>{extra_cond}</p:stCondLst><p:childTnLst/></p:cTn></p:par>"#,
        id = ids.next()
    ))
}

fn main_sequence(ids: &mut Ids) -> pml::CT_TLTimeNodeSequence {
    util::fragment(&format!(
        concat!(
            r#"<p:seq concurrent="1" nextAc="seek"><p:cTn id="{id}" dur="indefinite" nodeType="mainSeq"><p:childTnLst/></p:cTn>"#,
            r#"<p:prevCondLst><p:cond evt="onPrev" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst>"#,
            r#"<p:nextCondLst><p:cond evt="onNext" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq>"#
        ),
        id = ids.next()
    ))
}

/// The `tmRoot` node of a slide, created when missing.
pub(crate) fn root_list(slide: &mut pml::CT_Slide) -> (&mut pml::CT_TimeNodeList, Ids) {
    let timing = slide.timing.get_or_insert_with(Box::default);
    let ids = Ids(util::max_time_node_id(timing));
    let tn_lst = timing.tn_lst.get_or_insert_with(Box::default);
    let has_root = tn_lst.choice.iter().any(is_root);
    let mut ids = ids;
    if !has_root {
        let root: pml::CT_TLTimeNodeParallel = util::fragment(&format!(
            r#"<p:par><p:cTn id="{}" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst/></p:cTn></p:par>"#,
            ids.next()
        ));
        tn_lst
            .choice
            .insert(0, pml::CT_TimeNodeList_Choice::Par(Box::new(root)));
    }
    let root = tn_lst
        .choice
        .iter_mut()
        .find_map(|c| match c {
            pml::CT_TimeNodeList_Choice::Par(p) if is_root_ctn(p.c_tn.as_deref()) => p.c_tn.as_deref_mut(),
            _ => None,
        })
        .expect("root exists");
    (root.child_tn_lst.get_or_insert_with(Box::default), ids)
}

fn is_root_ctn(c: Option<&pml::CT_TLCommonTimeNodeData>) -> bool {
    c.and_then(|c| c.node_type) == Some(pml::ST_TLTimeNodeType::TmRoot)
}

fn is_root(c: &pml::CT_TimeNodeList_Choice) -> bool {
    matches!(c, pml::CT_TimeNodeList_Choice::Par(p) if is_root_ctn(p.c_tn.as_deref()))
}

fn is_main_seq(c: &pml::CT_TimeNodeList_Choice) -> bool {
    matches!(c, pml::CT_TimeNodeList_Choice::Seq(s)
        if s.c_tn.as_ref().and_then(|c| c.node_type) == Some(pml::ST_TLTimeNodeType::MainSeq))
}

fn root_ref(slide: &pml::CT_Slide) -> Option<&pml::CT_TimeNodeList> {
    slide
        .timing
        .as_ref()?
        .tn_lst
        .as_ref()?
        .choice
        .iter()
        .find_map(|c| match c {
            pml::CT_TimeNodeList_Choice::Par(p) if is_root_ctn(p.c_tn.as_deref()) => {
                p.c_tn.as_ref()?.child_tn_lst.as_deref()
            }
            _ => None,
        })
}

fn children_mut(c: &mut pml::CT_TimeNodeList_Choice) -> Option<&mut Vec<pml::CT_TimeNodeList_Choice>> {
    let ctn = match c {
        pml::CT_TimeNodeList_Choice::Par(p) => p.c_tn.as_deref_mut(),
        pml::CT_TimeNodeList_Choice::Seq(s) => s.c_tn.as_deref_mut(),
        _ => None,
    }?;
    Some(&mut ctn.child_tn_lst.get_or_insert_with(Box::default).choice)
}

fn children(c: &pml::CT_TimeNodeList_Choice) -> &[pml::CT_TimeNodeList_Choice] {
    let ctn = match c {
        pml::CT_TimeNodeList_Choice::Par(p) => p.c_tn.as_deref(),
        pml::CT_TimeNodeList_Choice::Seq(s) => s.c_tn.as_deref(),
        _ => None,
    };
    ctn.and_then(|c| c.child_tn_lst.as_deref())
        .map_or(&[], |l| l.choice.as_slice())
}

fn ctn(c: &pml::CT_TimeNodeList_Choice) -> Option<&pml::CT_TLCommonTimeNodeData> {
    match c {
        pml::CT_TimeNodeList_Choice::Par(p) => p.c_tn.as_deref(),
        pml::CT_TimeNodeList_Choice::Seq(s) => s.c_tn.as_deref(),
        _ => None,
    }
}

fn start_delay(c: &pml::CT_TLCommonTimeNodeData) -> Option<u32> {
    c.st_cond_lst
        .as_ref()?
        .cond
        .iter()
        .find_map(|cond| match cond.delay {
            Some(pml::ST_TLTime::Unsigned(d)) => Some(d),
            _ => None,
        })
}

fn raw_u32(e: &RawElement, name: &str) -> Option<u32> {
    e.attr(Ns::NONE, name)?.parse().ok()
}

/// Delay of a raw `cTn` (first `p:cond/@delay` of its start conditions).
fn raw_delay(ctn: &RawElement) -> u32 {
    ctn.child(Ns::P, "stCondLst")
        .and_then(|l| l.elements().find_map(|c| raw_u32(c, "delay")))
        .unwrap_or(0)
}

/// Length of an effect node in milliseconds: the latest end of its behaviours.
fn effect_length(effect: &pml::CT_TimeNodeList_Choice) -> u32 {
    let raw = util::node_raw(effect);
    let own = raw.child(Ns::P, "cTn");
    raw.descendants()
        .into_iter()
        .filter(|e| e.name.is(Ns::P, "cTn") && Some(*e) != own)
        .map(|e| raw_delay(e).saturating_add(raw_u32(e, "dur").unwrap_or(0)))
        .max()
        .unwrap_or(0)
}

/// Ends of a timed group: its start plus the longest effect (delay included).
fn group_end(group: &pml::CT_TimeNodeList_Choice) -> u32 {
    let start = ctn(group).and_then(start_delay).unwrap_or(0);
    let longest = children(group)
        .iter()
        .map(|e| {
            ctn(e)
                .and_then(start_delay)
                .unwrap_or(0)
                .saturating_add(effect_length(e))
        })
        .max()
        .unwrap_or(0);
    start.saturating_add(longest)
}

/// What kind of build entry a target needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuildKind {
    /// A shape with text (`p:bldP`).
    Paragraph,
    /// A graphic frame (`p:bldGraphic`).
    Graphic,
    /// No build entry (pictures, groups, connectors).
    None,
}

/// Adds an animation to the main sequence of `slide`.
pub(crate) fn add(slide: &mut pml::CT_Slide, spid: u32, build: BuildKind, a: &Animation) {
    let grp_id = next_group_id(slide, spid);
    let (root, mut ids) = root_list(slide);
    if !root.choice.iter().any(is_main_seq) {
        root.choice.insert(
            0,
            pml::CT_TimeNodeList_Choice::Seq(Box::new(main_sequence(&mut ids))),
        );
    }
    let main = root
        .choice
        .iter_mut()
        .find(|c| is_main_seq(c))
        .expect("main sequence exists");
    let main_id = ctn(main).and_then(|c| c.id).unwrap_or(0);
    let clicks = children_mut(main).expect("sequences have children");
    let effect = pml::CT_TimeNodeList_Choice::Par(Box::new(effect_par(&mut ids, spid, grp_id, a)));
    let new_click_group = a.trigger == Trigger::OnClick || clicks.is_empty();
    if new_click_group {
        // The first click group starts with the sequence when its first effect is not click-triggered.
        let extra = if clicks.is_empty() && a.trigger != Trigger::OnClick {
            format!(r#"<p:cond evt="onBegin" delay="0"><p:tn val="{main_id}"/></p:cond>"#)
        } else {
            String::new()
        };
        let mut click = pml::CT_TimeNodeList_Choice::Par(Box::new(group_par(&mut ids, "indefinite", &extra)));
        let mut timed = pml::CT_TimeNodeList_Choice::Par(Box::new(group_par(&mut ids, "0", "")));
        children_mut(&mut timed).expect("par").push(effect);
        children_mut(&mut click).expect("par").push(timed);
        clicks.push(click);
        return finish_build(slide, spid, grp_id, build);
    }
    let click = clicks.last_mut().expect("not empty");
    let timed_groups = children_mut(click).expect("par");
    match a.trigger {
        Trigger::WithPrevious if !timed_groups.is_empty() => {
            let last = timed_groups.last_mut().expect("not empty");
            children_mut(last).expect("par").push(effect);
        }
        _ => {
            let start = timed_groups.last().map_or(0, group_end);
            let mut timed =
                pml::CT_TimeNodeList_Choice::Par(Box::new(group_par(&mut ids, &start.to_string(), "")));
            children_mut(&mut timed).expect("par").push(effect);
            timed_groups.push(timed);
        }
    }
    finish_build(slide, spid, grp_id, build)
}

fn finish_build(slide: &mut pml::CT_Slide, spid: u32, grp_id: u32, build: BuildKind) {
    let entry = match build {
        BuildKind::Paragraph => pml::CT_BuildList_Choice::BldP(Box::new(pml::CT_TLBuildParagraph {
            spid: Some(spid),
            grp_id: Some(grp_id),
            anim_bg: Some(true),
            ..Default::default()
        })),
        BuildKind::Graphic => pml::CT_BuildList_Choice::BldGraphic(Box::new(util::fragment(&format!(
            r#"<p:bldGraphic spid="{spid}" grpId="{grp_id}"><p:bldAsOne/></p:bldGraphic>"#
        )))),
        BuildKind::None => return,
    };
    let timing = slide.timing.get_or_insert_with(Box::default);
    timing.bld_lst.get_or_insert_with(Box::default).choice.push(entry);
}

/// The next free `grpId` for animations of `spid`.
fn next_group_id(slide: &pml::CT_Slide, spid: u32) -> u32 {
    let Some(root) = root_ref(slide) else { return 0 };
    root.choice
        .iter()
        .filter(|c| is_main_seq(c))
        .flat_map(|m| {
            let raw = util::node_raw(m);
            raw.descendants()
                .into_iter()
                .filter(|e| e.name.is(Ns::P, "cTn") && e.attr(Ns::NONE, "presetClass").is_some())
                .filter(|e| {
                    e.child(Ns::P, "childTnLst")
                        .is_some_and(|l| util::raw_targets(l).contains(&spid))
                })
                .filter_map(|e| raw_u32(e, "grpId"))
                .collect::<Vec<_>>()
        })
        .map(|g| g + 1)
        .max()
        .unwrap_or(0)
}

/// Effect nodes of the main sequence with their trigger.
fn effects(slide: &pml::CT_Slide) -> Vec<&pml::CT_TimeNodeList_Choice> {
    let Some(root) = root_ref(slide) else {
        return Vec::new();
    };
    root.choice
        .iter()
        .filter(|c| is_main_seq(c))
        .flat_map(children)
        .flat_map(children)
        .flat_map(children)
        .collect()
}

fn describe(node: &pml::CT_TimeNodeList_Choice) -> Option<AnimationInfo> {
    let c = ctn(node)?;
    let raw = util::node_raw(node);
    let shape_id = *util::raw_targets(&raw).first()?;
    let class = match c.preset_class {
        Some(pml::ST_TLTimeNodePresetClassType::Entr) => EffectClass::Entrance,
        Some(pml::ST_TLTimeNodePresetClassType::Exit) => EffectClass::Exit,
        Some(pml::ST_TLTimeNodePresetClassType::Emph) => EffectClass::Emphasis,
        Some(pml::ST_TLTimeNodePresetClassType::Path) => EffectClass::MotionPath,
        _ => EffectClass::Other,
    };
    let preset_id = c.preset_id.unwrap_or(0);
    let direction = c.preset_subtype.and_then(Direction::from_subtype);
    let find_attr = |element: &str, attr: &str| {
        raw.descendants()
            .into_iter()
            .find(|e| e.name.is(Ns::P, element))
            .and_then(|e| e.attr(Ns::NONE, attr).and_then(|v| v.parse::<f64>().ok()))
    };
    let effect = match (class, preset_id) {
        (EffectClass::Entrance, 1) => Some(Effect::Appear),
        (EffectClass::Entrance, 10) => Some(Effect::FadeIn),
        (EffectClass::Entrance, 2) => direction.map(Effect::FlyIn),
        (EffectClass::Entrance, 22) => direction.map(Effect::WipeIn),
        (EffectClass::Exit, 1) => Some(Effect::Disappear),
        (EffectClass::Exit, 10) => Some(Effect::FadeOut),
        (EffectClass::Exit, 2) => direction.map(Effect::FlyOut),
        (EffectClass::Exit, 22) => direction.map(Effect::WipeOut),
        (EffectClass::Emphasis, 8) => find_attr("animRot", "by").map(|by| Effect::Spin(by / 60_000.0)),
        (EffectClass::Emphasis, 6) => raw
            .descendants()
            .into_iter()
            .find(|e| e.name.is(Ns::P, "animScale"))
            .and_then(|e| e.child(Ns::P, "by"))
            .and_then(|by| by.attr(Ns::NONE, "x")?.parse::<f64>().ok())
            .map(|x| Effect::GrowShrink(x / 100_000.0)),
        _ => None,
    };
    let trigger = match c.node_type {
        Some(pml::ST_TLTimeNodeType::WithEffect) => Trigger::WithPrevious,
        Some(pml::ST_TLTimeNodeType::AfterEffect) => Trigger::AfterPrevious,
        _ => Trigger::OnClick,
    };
    let duration_ms = if effect.is_some_and(Effect::is_instant) {
        0
    } else {
        effect_length(node)
    };
    Some(AnimationInfo {
        shape_id,
        class,
        preset_id,
        effect,
        trigger,
        delay_ms: start_delay(c).unwrap_or(0),
        duration_ms,
    })
}

/// Removes every animation, media node and build entry that targets one of
/// `shape_ids`; empty containers are removed with them.
pub(crate) fn remove_targets(slide: &mut pml::CT_Slide, shape_ids: &[u32]) {
    let targets_any = |c: &pml::CT_TimeNodeList_Choice| {
        util::raw_targets(&util::node_raw(c))
            .iter()
            .any(|id| shape_ids.contains(id))
    };
    let Some(timing) = slide.timing.as_deref_mut() else {
        return;
    };
    if let Some(bld) = timing.bld_lst.as_deref_mut() {
        bld.choice.retain(|b| {
            let spid = match b {
                pml::CT_BuildList_Choice::BldP(p) => p.spid,
                pml::CT_BuildList_Choice::BldDgm(d) => d.spid,
                pml::CT_BuildList_Choice::BldOleChart(o) => o.spid,
                pml::CT_BuildList_Choice::BldGraphic(g) => g.spid,
                pml::CT_BuildList_Choice::Other(_) => None,
            };
            !spid.is_some_and(|s| shape_ids.contains(&s))
        });
        if bld.choice.is_empty() {
            timing.bld_lst = None;
        }
    }
    let Some(tn_lst) = timing.tn_lst.as_deref_mut() else {
        return;
    };
    for top in &mut tn_lst.choice {
        if !is_root(top) {
            continue;
        }
        let Some(root) = children_mut(top) else { continue };
        for node in root.iter_mut() {
            if is_main_seq(node) {
                let clicks = children_mut(node).expect("sequence");
                for click in clicks.iter_mut() {
                    let timed = children_mut(click).expect("par");
                    for group in timed.iter_mut() {
                        if let Some(effects) = children_mut(group) {
                            effects.retain(|e| !targets_any(e));
                        }
                    }
                    timed.retain(|g| !children(g).is_empty());
                }
                clicks.retain(|c| !children(c).is_empty());
            }
        }
        root.retain(|n| {
            if is_main_seq(n) {
                !children(n).is_empty()
            } else {
                !targets_any(n)
            }
        });
    }
    tn_lst
        .choice
        .retain(|top| !is_root(top) || !children(top).is_empty());
    if tn_lst.choice.is_empty() {
        slide.timing = None;
    }
}

/// Removes the main sequence and the build list.
fn clear(slide: &mut pml::CT_Slide) {
    let Some(timing) = slide.timing.as_deref_mut() else {
        return;
    };
    timing.bld_lst = None;
    if let Some(tn_lst) = timing.tn_lst.as_deref_mut() {
        for top in &mut tn_lst.choice {
            if is_root(top)
                && let Some(root) = children_mut(top)
            {
                root.retain(|n| !is_main_seq(n));
            }
        }
        tn_lst
            .choice
            .retain(|top| !is_root(top) || !children(top).is_empty());
        if !tn_lst.choice.is_empty() {
            return;
        }
    }
    slide.timing = None;
}

impl Slide {
    /// The animations of the main sequence, in playing order.
    pub fn animations(&self) -> Vec<AnimationInfo> {
        effects(&self.data).into_iter().filter_map(describe).collect()
    }

    /// Identifiers of the shapes that have animations, without duplicates.
    pub fn animated_shapes(&self) -> Vec<u32> {
        let mut ids = Vec::new();
        for a in self.animations() {
            if !ids.contains(&a.shape_id) {
                ids.push(a.shape_id);
            }
        }
        ids
    }
}

impl SlideMut<'_> {
    /// Adds an animation to a top-level shape (or group), at the end of the
    /// slide's main sequence.
    ///
    /// ```
    /// use openxml_core::Length;
    /// use openxml_pptx::{Animation, Direction, Effect, LayoutKind, Presentation, ShapeType, Trigger};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// let id = slide.add_shape(ShapeType::Rect, Length::cm(2.0), Length::cm(2.0), Length::cm(5.0), Length::cm(3.0)).id();
    /// slide.add_animation(id, Animation::new(Effect::FlyIn(Direction::Left)))?;
    /// slide.add_animation(id, Animation::new(Effect::Spin(360.0)).trigger(Trigger::AfterPrevious).delay(250))?;
    /// let animations = slide.animations();
    /// assert_eq!(animations.len(), 2);
    /// assert_eq!(animations[1].effect, Some(Effect::Spin(360.0)));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn add_animation(&mut self, shape_id: u32, animation: Animation) -> Result<()> {
        let tree = self.tree_mut();
        let top_level = tree.choice.iter().find(|c| util::choice_id(c) == Some(shape_id));
        let build = match top_level {
            None if util::find(tree, shape_id).is_some() => {
                return Err(Error::InvalidArgument(format!(
                    "shape {shape_id} is inside a group; animate the group instead"
                )));
            }
            None => return Err(Error::NotFound(format!("shape {shape_id}"))),
            Some(pml::CT_GroupShape_Choice::Sp(sp)) if sp.tx_body.is_some() => BuildKind::Paragraph,
            Some(pml::CT_GroupShape_Choice::GraphicFrame(_)) => BuildKind::Graphic,
            Some(_) => BuildKind::None,
        };
        add(self.raw_mut(), shape_id, build, &animation);
        Ok(())
    }

    /// Removes all animations of a shape.
    pub fn remove_animations(&mut self, shape_id: u32) {
        remove_targets(self.raw_mut(), &[shape_id]);
    }

    /// Removes the main sequence and all build entries (media playback nodes stay).
    pub fn clear_animations(&mut self) {
        clear(self.raw_mut());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slide() -> pml::CT_Slide {
        pml::CT_Slide::default()
    }

    #[test]
    fn directions_and_presets() {
        for d in [
            Direction::Bottom,
            Direction::Left,
            Direction::Right,
            Direction::Top,
        ] {
            assert_eq!(Direction::from_subtype(d.subtype()), Some(d));
            assert_ne!(d.wipe_in(), d.wipe_out());
        }
        assert_eq!(Direction::from_subtype(3), None);
        assert_eq!(Effect::FadeIn.preset(), (10, 0));
        assert_eq!(Effect::FlyOut(Direction::Left).preset(), (2, 8));
        assert_eq!(Effect::WipeIn(Direction::Top).preset(), (22, 1));
        assert_eq!(Effect::Spin(90.0).class(), EffectClass::Emphasis);
        assert_eq!(Effect::Disappear.preset_class(), "exit");
        assert_eq!(Animation::new(Effect::GrowShrink(2.0)).duration_ms, 2000);
        assert_eq!(Animation::new(Effect::Appear).duration_ms, 500);
        assert_eq!(Trigger::WithPrevious.node_type(), "withEffect");
    }

    #[test]
    fn behaviours_of_every_effect_parse() {
        let effects = [
            Effect::Appear,
            Effect::FadeIn,
            Effect::FlyIn(Direction::Bottom),
            Effect::WipeIn(Direction::Right),
            Effect::Disappear,
            Effect::FadeOut,
            Effect::FlyOut(Direction::Top),
            Effect::WipeOut(Direction::Left),
            Effect::Spin(-90.0),
            Effect::GrowShrink(0.5),
        ];
        for e in effects {
            let mut ids = Ids(10);
            let par = effect_par(&mut ids, 7, 2, &Animation::new(e).delay(100));
            let node = pml::CT_TimeNodeList_Choice::Par(Box::new(par));
            let info = describe(&node).unwrap();
            assert_eq!(info.effect, Some(e), "{e:?}");
            assert_eq!(info.shape_id, 7);
            assert_eq!(info.delay_ms, 100);
            let raw = util::node_raw(&node);
            let ids_used = raw
                .descendants()
                .into_iter()
                .filter(|e| &*e.name.local == "cTn")
                .count() as u32;
            assert_eq!(ids.0, 10 + ids_used, "one id per time node");
        }
    }

    #[test]
    fn sequences_are_built_and_timed() {
        let mut s = slide();
        add(
            &mut s,
            2,
            BuildKind::Paragraph,
            &Animation::new(Effect::FadeIn).duration(400),
        );
        add(
            &mut s,
            3,
            BuildKind::None,
            &Animation::new(Effect::FadeIn)
                .trigger(Trigger::WithPrevious)
                .delay(300),
        );
        add(
            &mut s,
            2,
            BuildKind::Paragraph,
            &Animation::new(Effect::Spin(90.0)).trigger(Trigger::AfterPrevious),
        );
        let root = root_ref(&s).unwrap();
        let main = root.choice.iter().find(|c| is_main_seq(c)).unwrap();
        let clicks = children(main);
        assert_eq!(clicks.len(), 1);
        let timed = children(&clicks[0]);
        assert_eq!(timed.len(), 2);
        // The after-previous group starts when the longest effect ends: 300 ms delay + 500 ms.
        assert_eq!(ctn(&timed[1]).and_then(start_delay), Some(800));
        assert_eq!(group_end(&timed[0]), 800);
        assert_eq!(next_group_id(&s, 2), 2);
        assert_eq!(next_group_id(&s, 3), 1);
        assert_eq!(next_group_id(&s, 9), 0);
        let builds = &s.timing.as_ref().unwrap().bld_lst.as_ref().unwrap().choice;
        assert_eq!(builds.len(), 2, "shape 3 has no text and gets no build entry");

        let infos: Vec<AnimationInfo> = effects(&s).into_iter().filter_map(describe).collect();
        assert_eq!(infos.len(), 3);
        assert_eq!(infos[1].trigger, Trigger::WithPrevious);
        assert_eq!(infos[2].duration_ms, 2000);

        remove_targets(&mut s, &[3]);
        assert_eq!(effects(&s).len(), 2);
        remove_targets(&mut s, &[2]);
        assert!(s.timing.is_none(), "an empty timing tree is removed");
    }

    #[test]
    fn clearing_keeps_other_timing_nodes() {
        let mut s = slide();
        add(&mut s, 2, BuildKind::Graphic, &Animation::new(Effect::Appear));
        let (root, mut ids) = root_list(&mut s);
        root.choice.push(pml::CT_TimeNodeList_Choice::Par(Box::new(util::fragment(&format!(
            r#"<p:par><p:cTn id="{}" fill="hold"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst/></p:cTn></p:par>"#,
            ids.next()
        )))));
        clear(&mut s);
        let root = root_ref(&s).unwrap();
        assert_eq!(root.choice.len(), 1);
        assert!(s.timing.as_ref().unwrap().bld_lst.is_none());
        clear(&mut pml::CT_Slide::default());
    }
}
