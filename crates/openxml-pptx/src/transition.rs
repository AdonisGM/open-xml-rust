//! Slide transitions (`p:transition`, the ECMA-376 set).
//!
//! PowerPoint 2010+ effects (`p14:*`) live inside `mc:AlternateContent`,
//! which the strict ECMA schema does not allow on `p:sld`; this module
//! therefore only writes ECMA transitions, reads the ECMA fallback of
//! existing alternate content, and replaces such content when a new
//! transition is set.

use openxml_schema::pml;
use openxml_xml::{Ns, RawElement};

use crate::presentation::Presentation;
use crate::slide::{Slide, SlideMut};

/// Speed of a transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TransitionSpeed {
    /// Slow (about 1 s).
    Slow,
    /// Medium (about 0.75 s).
    Medium,
    /// Fast (about 0.5 s), the default.
    #[default]
    Fast,
}

/// Orientation of bar-like transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Horizontal.
    Horizontal,
    /// Vertical.
    Vertical,
}

/// One of the four sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SideDirection {
    /// Left.
    Left,
    /// Up.
    Up,
    /// Right.
    Right,
    /// Down.
    Down,
}

/// One of the four corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CornerDirection {
    /// Left-up.
    LeftUp,
    /// Right-up.
    RightUp,
    /// Left-down.
    LeftDown,
    /// Right-down.
    RightDown,
}

/// A side or a corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EightDirection {
    /// A side.
    Side(SideDirection),
    /// A corner.
    Corner(CornerDirection),
}

/// A transition effect of ECMA-376 (§19.7.x of Part 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransitionEffect {
    /// Venetian blinds.
    Blinds(Orientation),
    /// Checkerboard.
    Checker(Orientation),
    /// Circle.
    Circle,
    /// Comb.
    Comb(Orientation),
    /// The new slide covers the old one.
    Cover(EightDirection),
    /// Cut, optionally through black.
    Cut {
        /// Cut through a black screen.
        through_black: bool,
    },
    /// Diamond.
    Diamond,
    /// Dissolve.
    Dissolve,
    /// Fade, optionally through black.
    Fade {
        /// Fade through a black screen.
        through_black: bool,
    },
    /// Newsflash.
    Newsflash,
    /// Plus.
    Plus,
    /// The old slide is pulled away.
    Pull(EightDirection),
    /// The new slide pushes the old one.
    Push(SideDirection),
    /// Random transition.
    Random,
    /// Random bars.
    RandomBars(Orientation),
    /// Split.
    Split {
        /// Orientation of the split.
        orientation: Orientation,
        /// Split outwards (`true`) or inwards.
        outward: bool,
    },
    /// Diagonal strips.
    Strips(CornerDirection),
    /// Wedge.
    Wedge,
    /// Wheel with the given number of spokes.
    Wheel {
        /// Number of spokes (1–…).
        spokes: u32,
    },
    /// Wipe.
    Wipe(SideDirection),
    /// Zoom in (`outward: false`) or out.
    Zoom {
        /// Zoom out.
        outward: bool,
    },
}

/// Transition settings of a slide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Transition {
    /// The visual effect; `None` for an immediate change.
    pub effect: Option<TransitionEffect>,
    /// Speed of the effect.
    pub speed: TransitionSpeed,
    /// Whether a click advances to the next slide (default `true`).
    pub advance_on_click: bool,
    /// Advance automatically after this many milliseconds.
    pub advance_after_ms: Option<u32>,
}

impl Transition {
    /// A transition with the given effect, advancing on click.
    pub fn new(effect: TransitionEffect) -> Transition {
        Transition {
            effect: Some(effect),
            speed: TransitionSpeed::Fast,
            advance_on_click: true,
            advance_after_ms: None,
        }
    }

    /// Sets the speed.
    pub fn speed(mut self, speed: TransitionSpeed) -> Self {
        self.speed = speed;
        self
    }

    /// Sets whether a click advances the slide.
    pub fn advance_on_click(mut self, on: bool) -> Self {
        self.advance_on_click = on;
        self
    }

    /// Advances automatically after `ms` milliseconds.
    pub fn advance_after(mut self, ms: u32) -> Self {
        self.advance_after_ms = Some(ms);
        self
    }

    pub(crate) fn to_pml(self) -> pml::CT_SlideTransition {
        pml::CT_SlideTransition {
            spd: match self.speed {
                TransitionSpeed::Fast => None,
                TransitionSpeed::Medium => Some(pml::ST_TransitionSpeed::Med),
                TransitionSpeed::Slow => Some(pml::ST_TransitionSpeed::Slow),
            },
            adv_click: (!self.advance_on_click).then_some(false),
            adv_tm: self.advance_after_ms,
            choice: self.effect.map(effect_to_pml),
            ..Default::default()
        }
    }

    pub(crate) fn from_pml(t: &pml::CT_SlideTransition) -> Transition {
        Transition {
            effect: t.choice.as_ref().and_then(effect_from_pml),
            speed: match t.spd {
                Some(pml::ST_TransitionSpeed::Slow) => TransitionSpeed::Slow,
                Some(pml::ST_TransitionSpeed::Med) => TransitionSpeed::Medium,
                _ => TransitionSpeed::Fast,
            },
            advance_on_click: t.adv_click.unwrap_or(true),
            advance_after_ms: t.adv_tm,
        }
    }
}

fn orientation(o: Orientation) -> pml::CT_OrientationTransition {
    pml::CT_OrientationTransition {
        dir: match o {
            Orientation::Horizontal => None,
            Orientation::Vertical => Some(pml::ST_Direction::Vert),
        },
        ..Default::default()
    }
}

fn orientation_of(d: Option<pml::ST_Direction>) -> Orientation {
    match d {
        Some(pml::ST_Direction::Vert) => Orientation::Vertical,
        _ => Orientation::Horizontal,
    }
}

fn side(d: SideDirection) -> pml::ST_TransitionSideDirectionType {
    match d {
        SideDirection::Left => pml::ST_TransitionSideDirectionType::L,
        SideDirection::Up => pml::ST_TransitionSideDirectionType::U,
        SideDirection::Right => pml::ST_TransitionSideDirectionType::R,
        SideDirection::Down => pml::ST_TransitionSideDirectionType::D,
    }
}

fn side_of(d: pml::ST_TransitionSideDirectionType) -> SideDirection {
    match d {
        pml::ST_TransitionSideDirectionType::L => SideDirection::Left,
        pml::ST_TransitionSideDirectionType::U => SideDirection::Up,
        pml::ST_TransitionSideDirectionType::R => SideDirection::Right,
        pml::ST_TransitionSideDirectionType::D => SideDirection::Down,
    }
}

fn corner(d: CornerDirection) -> pml::ST_TransitionCornerDirectionType {
    match d {
        CornerDirection::LeftUp => pml::ST_TransitionCornerDirectionType::Lu,
        CornerDirection::RightUp => pml::ST_TransitionCornerDirectionType::Ru,
        CornerDirection::LeftDown => pml::ST_TransitionCornerDirectionType::Ld,
        CornerDirection::RightDown => pml::ST_TransitionCornerDirectionType::Rd,
    }
}

fn corner_of(d: pml::ST_TransitionCornerDirectionType) -> CornerDirection {
    match d {
        pml::ST_TransitionCornerDirectionType::Lu => CornerDirection::LeftUp,
        pml::ST_TransitionCornerDirectionType::Ru => CornerDirection::RightUp,
        pml::ST_TransitionCornerDirectionType::Ld => CornerDirection::LeftDown,
        pml::ST_TransitionCornerDirectionType::Rd => CornerDirection::RightDown,
    }
}

fn eight(d: EightDirection) -> pml::CT_EightDirectionTransition {
    pml::CT_EightDirectionTransition {
        dir: Some(match d {
            EightDirection::Side(s) => {
                pml::ST_TransitionEightDirectionType::TransitionSideDirectionType(side(s))
            }
            EightDirection::Corner(c) => {
                pml::ST_TransitionEightDirectionType::TransitionCornerDirectionType(corner(c))
            }
        }),
        ..Default::default()
    }
}

fn eight_of(d: Option<&pml::ST_TransitionEightDirectionType>) -> EightDirection {
    match d {
        Some(pml::ST_TransitionEightDirectionType::TransitionSideDirectionType(s)) => {
            EightDirection::Side(side_of(*s))
        }
        Some(pml::ST_TransitionEightDirectionType::TransitionCornerDirectionType(c)) => {
            EightDirection::Corner(corner_of(*c))
        }
        None => EightDirection::Side(SideDirection::Left),
    }
}

fn black(through_black: bool) -> pml::CT_OptionalBlackTransition {
    pml::CT_OptionalBlackTransition {
        thru_blk: through_black.then_some(true),
        ..Default::default()
    }
}

fn effect_to_pml(e: TransitionEffect) -> pml::CT_SlideTransition_Choice {
    use TransitionEffect as E;
    use pml::CT_SlideTransition_Choice as T;
    match e {
        E::Blinds(o) => T::Blinds(Box::new(orientation(o))),
        E::Checker(o) => T::Checker(Box::new(orientation(o))),
        E::Circle => T::Circle(Box::default()),
        E::Comb(o) => T::Comb(Box::new(orientation(o))),
        E::Cover(d) => T::Cover(Box::new(eight(d))),
        E::Cut { through_black } => T::Cut(Box::new(black(through_black))),
        E::Diamond => T::Diamond(Box::default()),
        E::Dissolve => T::Dissolve(Box::default()),
        E::Fade { through_black } => T::Fade(Box::new(black(through_black))),
        E::Newsflash => T::Newsflash(Box::default()),
        E::Plus => T::Plus(Box::default()),
        E::Pull(d) => T::Pull(Box::new(eight(d))),
        E::Push(d) => T::Push(Box::new(pml::CT_SideDirectionTransition {
            dir: Some(side(d)),
            ..Default::default()
        })),
        E::Random => T::Random(Box::default()),
        E::RandomBars(o) => T::RandomBar(Box::new(orientation(o))),
        E::Split {
            orientation: o,
            outward,
        } => T::Split(Box::new(pml::CT_SplitTransition {
            orient: Some(match o {
                Orientation::Horizontal => pml::ST_Direction::Horz,
                Orientation::Vertical => pml::ST_Direction::Vert,
            }),
            dir: Some(if outward {
                pml::ST_TransitionInOutDirectionType::Out
            } else {
                pml::ST_TransitionInOutDirectionType::In
            }),
            ..Default::default()
        })),
        E::Strips(d) => T::Strips(Box::new(pml::CT_CornerDirectionTransition {
            dir: Some(corner(d)),
            ..Default::default()
        })),
        E::Wedge => T::Wedge(Box::default()),
        E::Wheel { spokes } => T::Wheel(Box::new(pml::CT_WheelTransition {
            spokes: Some(spokes.max(1)),
            ..Default::default()
        })),
        E::Wipe(d) => T::Wipe(Box::new(pml::CT_SideDirectionTransition {
            dir: Some(side(d)),
            ..Default::default()
        })),
        E::Zoom { outward } => T::Zoom(Box::new(pml::CT_InOutTransition {
            dir: Some(if outward {
                pml::ST_TransitionInOutDirectionType::Out
            } else {
                pml::ST_TransitionInOutDirectionType::In
            }),
            ..Default::default()
        })),
    }
}

fn effect_from_pml(c: &pml::CT_SlideTransition_Choice) -> Option<TransitionEffect> {
    use TransitionEffect as E;
    use pml::CT_SlideTransition_Choice as T;
    Some(match c {
        T::Blinds(o) => E::Blinds(orientation_of(o.dir)),
        T::Checker(o) => E::Checker(orientation_of(o.dir)),
        T::Circle(_) => E::Circle,
        T::Comb(o) => E::Comb(orientation_of(o.dir)),
        T::Cover(d) => E::Cover(eight_of(d.dir.as_ref())),
        T::Cut(b) => E::Cut {
            through_black: b.thru_blk.unwrap_or(false),
        },
        T::Diamond(_) => E::Diamond,
        T::Dissolve(_) => E::Dissolve,
        T::Fade(b) => E::Fade {
            through_black: b.thru_blk.unwrap_or(false),
        },
        T::Newsflash(_) => E::Newsflash,
        T::Plus(_) => E::Plus,
        T::Pull(d) => E::Pull(eight_of(d.dir.as_ref())),
        T::Push(d) => E::Push(d.dir.map_or(SideDirection::Left, side_of)),
        T::Random(_) => E::Random,
        T::RandomBar(o) => E::RandomBars(orientation_of(o.dir)),
        T::Split(s) => E::Split {
            orientation: orientation_of(s.orient),
            outward: s.dir != Some(pml::ST_TransitionInOutDirectionType::In),
        },
        T::Strips(d) => E::Strips(d.dir.map_or(CornerDirection::LeftUp, corner_of)),
        T::Wedge(_) => E::Wedge,
        T::Wheel(w) => E::Wheel {
            spokes: w.spokes.unwrap_or(4),
        },
        T::Wipe(d) => E::Wipe(d.dir.map_or(SideDirection::Left, side_of)),
        T::Zoom(z) => E::Zoom {
            outward: z.dir == Some(pml::ST_TransitionInOutDirectionType::Out),
        },
        T::Other(_) => return None,
    })
}

/// Whether a raw element is `mc:AlternateContent` offering a `p:transition`.
fn is_transition_alternate(raw: &RawElement) -> bool {
    raw.name.is(Ns::MC, "AlternateContent")
        && raw
            .elements()
            .any(|branch| branch.elements().any(|e| e.name.is(Ns::P, "transition")))
}

/// The ECMA `p:transition` of the fallback branch of an alternate content element.
fn fallback_transition(raw: &RawElement) -> Option<pml::CT_SlideTransition> {
    let fallback = raw.child(Ns::MC, "Fallback")?;
    fallback.child(Ns::P, "transition")?.to_typed().ok()
}

impl Slide {
    /// The transition into this slide. For PowerPoint 2010+ transitions
    /// stored as alternate content, the ECMA fallback is described.
    pub fn transition(&self) -> Option<Transition> {
        if let Some(t) = &self.data.transition {
            return Some(Transition::from_pml(t));
        }
        self.data
            .extra_children
            .iter()
            .filter(|x| is_transition_alternate(&x.element))
            .find_map(|x| fallback_transition(&x.element))
            .map(|t| Transition::from_pml(&t))
    }
}

impl SlideMut<'_> {
    /// Sets (or with `None` removes) the transition into this slide. Any
    /// PowerPoint 2010+ transition stored as alternate content is replaced.
    ///
    /// ```
    /// use openxml_pptx::{LayoutKind, Presentation, SideDirection, Transition, TransitionEffect, TransitionSpeed};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// let t = Transition::new(TransitionEffect::Push(SideDirection::Up))
    ///     .speed(TransitionSpeed::Slow)
    ///     .advance_after(3000);
    /// slide.set_transition(Some(t));
    /// assert_eq!(slide.transition(), Some(t));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn set_transition(&mut self, transition: Option<Transition>) {
        let data = self.raw_mut();
        data.extra_children
            .retain(|x| !is_transition_alternate(&x.element));
        data.transition = transition.map(|t| Box::new(t.to_pml()));
    }
}

impl Presentation {
    /// Applies the same transition to every slide.
    pub fn set_transition_all(&mut self, transition: Option<Transition>) {
        for i in 0..self.slide_count() {
            if let Some(mut s) = self.slide_mut(i) {
                s.set_transition(transition);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_round_trips() {
        let dirs = [
            SideDirection::Left,
            SideDirection::Up,
            SideDirection::Right,
            SideDirection::Down,
        ];
        let corners = [
            CornerDirection::LeftUp,
            CornerDirection::RightUp,
            CornerDirection::LeftDown,
            CornerDirection::RightDown,
        ];
        let mut effects = vec![
            TransitionEffect::Circle,
            TransitionEffect::Diamond,
            TransitionEffect::Dissolve,
            TransitionEffect::Newsflash,
            TransitionEffect::Plus,
            TransitionEffect::Random,
            TransitionEffect::Wedge,
            TransitionEffect::Wheel { spokes: 8 },
            TransitionEffect::Zoom { outward: true },
            TransitionEffect::Zoom { outward: false },
            TransitionEffect::Cut { through_black: true },
            TransitionEffect::Fade { through_black: false },
        ];
        for o in [Orientation::Horizontal, Orientation::Vertical] {
            effects.extend([
                TransitionEffect::Blinds(o),
                TransitionEffect::Checker(o),
                TransitionEffect::Comb(o),
                TransitionEffect::RandomBars(o),
                TransitionEffect::Split {
                    orientation: o,
                    outward: true,
                },
                TransitionEffect::Split {
                    orientation: o,
                    outward: false,
                },
            ]);
        }
        for d in dirs {
            effects.extend([
                TransitionEffect::Push(d),
                TransitionEffect::Wipe(d),
                TransitionEffect::Cover(EightDirection::Side(d)),
                TransitionEffect::Pull(EightDirection::Side(d)),
            ]);
        }
        for c in corners {
            effects.extend([
                TransitionEffect::Strips(c),
                TransitionEffect::Cover(EightDirection::Corner(c)),
                TransitionEffect::Pull(EightDirection::Corner(c)),
            ]);
        }
        for e in effects {
            for speed in [
                TransitionSpeed::Slow,
                TransitionSpeed::Medium,
                TransitionSpeed::Fast,
            ] {
                let t = Transition::new(e)
                    .speed(speed)
                    .advance_on_click(false)
                    .advance_after(1500);
                assert_eq!(Transition::from_pml(&t.to_pml()), t);
            }
        }
        let none = Transition::default();
        assert_eq!(Transition::from_pml(&none.to_pml()).effect, None);
    }

    #[test]
    fn alternate_content_fallback_is_read() {
        let raw = RawElement::parse(concat!(
            r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" "#,
            r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" "#,
            r#"xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main">"#,
            r#"<mc:Choice Requires="p14"><p:transition spd="slow" p14:dur="2000"><p14:prism/></p:transition></mc:Choice>"#,
            r#"<mc:Fallback><p:transition spd="slow"><p:fade/></p:transition></mc:Fallback></mc:AlternateContent>"#
        ))
        .unwrap();
        assert!(is_transition_alternate(&raw));
        let t = Transition::from_pml(&fallback_transition(&raw).unwrap());
        assert_eq!(t.effect, Some(TransitionEffect::Fade { through_black: false }));
        assert_eq!(t.speed, TransitionSpeed::Slow);
    }
}
