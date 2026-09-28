//! Presentation-level settings: slide size presets, slide show settings
//! (`presProps.xml`), custom shows and custom document properties.

use openxml_core::part::{read_related, write_part};
use openxml_core::{Error, Length, Result};
use openxml_opc::PartName;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::{pml, shared_custom_properties as cp, shared_extended_properties as ep};

use crate::presentation::Presentation;

/// Slide sizes offered by PowerPoint's "Slide Size" dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlideSize {
    /// Widescreen 16:9, 13.333 × 7.5 in (the default of new presentations).
    Widescreen,
    /// Standard 4:3, 10 × 7.5 in.
    Standard,
    /// On-screen show 16:9, 10 × 5.625 in.
    OnScreen16x9,
    /// On-screen show 16:10, 10 × 6.25 in.
    OnScreen16x10,
    /// Letter paper, 10 × 7.5 in.
    Letter,
    /// A4 paper, 10.83 × 7.5 in.
    A4,
    /// 35 mm slides, 11.25 × 7.5 in.
    Slide35mm,
    /// Overhead, 10 × 7.5 in.
    Overhead,
    /// Banner, 8 × 1 in.
    Banner,
    /// Any other size.
    Custom(Length, Length),
}

impl SlideSize {
    /// Width, height and `ST_SlideSizeType` of the preset.
    fn spec(self) -> (i64, i64, Option<pml::ST_SlideSizeType>) {
        use pml::ST_SlideSizeType as T;
        match self {
            SlideSize::Widescreen => (12_192_000, 6_858_000, None),
            SlideSize::Standard => (9_144_000, 6_858_000, Some(T::Screen4x3)),
            SlideSize::OnScreen16x9 => (9_144_000, 5_143_500, Some(T::Screen16x9)),
            SlideSize::OnScreen16x10 => (9_144_000, 5_715_000, Some(T::Screen16x10)),
            SlideSize::Letter => (9_144_000, 6_858_000, Some(T::Letter)),
            SlideSize::A4 => (9_906_000, 6_858_000, Some(T::A4)),
            SlideSize::Slide35mm => (10_287_000, 6_858_000, Some(T::V35mm)),
            SlideSize::Overhead => (9_144_000, 6_858_000, Some(T::Overhead)),
            SlideSize::Banner => (7_315_200, 914_400, Some(T::Banner)),
            SlideSize::Custom(w, h) => (w.as_emu(), h.as_emu(), Some(T::Custom)),
        }
    }

    /// Width and height.
    pub fn dimensions(self) -> (Length, Length) {
        let (w, h, _) = self.spec();
        (Length::emu(w), Length::emu(h))
    }

    fn from_pml(sz: &pml::CT_SlideSize) -> SlideSize {
        let (w, h) = (i64::from(sz.cx.unwrap_or(0)), i64::from(sz.cy.unwrap_or(0)));
        let presets = [
            SlideSize::Widescreen,
            SlideSize::Standard,
            SlideSize::OnScreen16x9,
            SlideSize::OnScreen16x10,
            SlideSize::Letter,
            SlideSize::A4,
            SlideSize::Slide35mm,
            SlideSize::Overhead,
            SlideSize::Banner,
        ];
        presets
            .into_iter()
            .find(|p| {
                let (pw, ph, t) = p.spec();
                pw == w && ph == h && t == sz.type_
            })
            .unwrap_or(SlideSize::Custom(Length::emu(w), Length::emu(h)))
    }
}

/// How the slide show is presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShowType {
    /// Presented by a speaker (full screen).
    Speaker,
    /// Browsed by an individual (window), optionally with a scroll bar.
    Window {
        /// Show the scroll bar.
        scrollbar: bool,
    },
    /// Browsed at a kiosk (full screen, loops, restarts after inactivity).
    Kiosk {
        /// Restart after this many milliseconds of inactivity.
        restart_after_ms: Option<u32>,
    },
}

/// Which slides the show presents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShowSlides {
    /// All slides.
    All,
    /// A range of slides (1-based, inclusive, as in PowerPoint).
    Range(u32, u32),
    /// A custom show, by identifier.
    CustomShow(u32),
}

/// Slide show settings (`p:showPr` in `presProps.xml`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShowSettings {
    /// How the show is presented.
    pub show_type: ShowType,
    /// Which slides are shown.
    pub slides: ShowSlides,
    /// Loop continuously until Esc.
    pub loop_until_esc: bool,
    /// Play narrations.
    pub narration: bool,
    /// Play animations.
    pub animations: bool,
    /// Advance slides using saved timings.
    pub use_timings: bool,
}

impl Default for ShowSettings {
    fn default() -> Self {
        ShowSettings {
            show_type: ShowType::Speaker,
            slides: ShowSlides::All,
            loop_until_esc: false,
            narration: true,
            animations: true,
            use_timings: true,
        }
    }
}

impl ShowSettings {
    fn from_pml(p: &pml::CT_ShowProperties) -> ShowSettings {
        ShowSettings {
            show_type: match &p.show_type {
                Some(pml::EG_ShowType::Browse(b)) => ShowType::Window {
                    scrollbar: b.show_scrollbar.unwrap_or(true),
                },
                Some(pml::EG_ShowType::Kiosk(k)) => ShowType::Kiosk {
                    restart_after_ms: k.restart,
                },
                _ => ShowType::Speaker,
            },
            slides: match &p.slide_list_choice {
                Some(pml::EG_SlideListChoice::SldRg(r)) => {
                    ShowSlides::Range(r.st.unwrap_or(1), r.end.unwrap_or(1))
                }
                Some(pml::EG_SlideListChoice::CustShow(c)) => ShowSlides::CustomShow(c.id.unwrap_or(0)),
                _ => ShowSlides::All,
            },
            loop_until_esc: p.loop_.unwrap_or(false),
            narration: p.show_narration.unwrap_or(false),
            animations: p.show_animation.unwrap_or(false),
            use_timings: p.use_timings.unwrap_or(true),
        }
    }

    fn apply(self, p: &mut pml::CT_ShowProperties) {
        // Kiosk mode implies looping; PowerPoint stores the flag as well.
        let looping = self.loop_until_esc || matches!(self.show_type, ShowType::Kiosk { .. });
        p.loop_ = looping.then_some(true);
        p.show_narration = Some(self.narration);
        p.show_animation = Some(self.animations);
        p.use_timings = (!self.use_timings).then_some(false);
        p.show_type = Some(match self.show_type {
            ShowType::Speaker => pml::EG_ShowType::Present(Box::default()),
            ShowType::Window { scrollbar } => pml::EG_ShowType::Browse(Box::new(pml::CT_ShowInfoBrowse {
                show_scrollbar: (!scrollbar).then_some(false),
                ..Default::default()
            })),
            ShowType::Kiosk { restart_after_ms } => {
                pml::EG_ShowType::Kiosk(Box::new(pml::CT_ShowInfoKiosk {
                    restart: restart_after_ms,
                    ..Default::default()
                }))
            }
        });
        p.slide_list_choice = Some(match self.slides {
            ShowSlides::All => pml::EG_SlideListChoice::SldAll(Box::default()),
            ShowSlides::Range(st, end) => pml::EG_SlideListChoice::SldRg(Box::new(pml::CT_IndexRange {
                st: Some(st.max(1)),
                end: Some(end.max(st.max(1))),
                ..Default::default()
            })),
            ShowSlides::CustomShow(id) => pml::EG_SlideListChoice::CustShow(Box::new(pml::CT_CustomShowId {
                id: Some(id),
                ..Default::default()
            })),
        });
    }
}

/// A custom show: a named subset of slides in a chosen order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomShow {
    /// Identifier (unique within the presentation).
    pub id: u32,
    /// Name.
    pub name: String,
    /// Slides, as positions in the presentation (0-based).
    pub slides: Vec<usize>,
}

/// The value of a custom document property.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    /// Text.
    Text(String),
    /// A 32-bit integer.
    Integer(i32),
    /// A floating-point number.
    Number(f64),
    /// Yes / no.
    Bool(bool),
    /// A date and time in ISO 8601 form (`2024-05-01T12:00:00Z`).
    Date(String),
}

impl PropertyValue {
    fn to_pml(&self) -> cp::CT_Property_Choice {
        match self {
            PropertyValue::Text(s) => cp::CT_Property_Choice::Lpwstr(s.clone()),
            PropertyValue::Integer(i) => cp::CT_Property_Choice::I4(*i),
            PropertyValue::Number(n) => cp::CT_Property_Choice::R8(*n),
            PropertyValue::Bool(b) => cp::CT_Property_Choice::Bool(*b),
            PropertyValue::Date(d) => cp::CT_Property_Choice::Filetime(d.clone()),
        }
    }

    fn from_pml(c: &cp::CT_Property_Choice) -> Option<PropertyValue> {
        use cp::CT_Property_Choice as P;
        Some(match c {
            P::Lpwstr(s) | P::Lpstr(s) | P::Bstr(s) => PropertyValue::Text(s.clone()),
            P::I4(i) | P::Int(i) => PropertyValue::Integer(*i),
            P::I1(i) => PropertyValue::Integer(i32::from(*i)),
            P::I2(i) => PropertyValue::Integer(i32::from(*i)),
            P::Ui1(i) => PropertyValue::Integer(i32::from(*i)),
            P::Ui2(i) => PropertyValue::Integer(i32::from(*i)),
            P::R8(n) | P::Decimal(n) => PropertyValue::Number(*n),
            P::R4(n) => PropertyValue::Number(f64::from(*n)),
            P::Bool(b) => PropertyValue::Bool(*b),
            P::Filetime(d) | P::Date(d) => PropertyValue::Date(d.clone()),
            _ => return None,
        })
    }
}

/// Format identifier of user-defined custom properties.
const CUSTOM_FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";

impl Presentation {
    // ----- slide size -------------------------------------------------------------

    /// Sets the slide size from a preset (shapes are not rescaled).
    ///
    /// ```
    /// use openxml_pptx::{Presentation, SlideSize};
    ///
    /// let mut deck = Presentation::new();
    /// deck.set_slide_size_preset(SlideSize::A4)?;
    /// assert_eq!(deck.slide_size_preset(), SlideSize::A4);
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn set_slide_size_preset(&mut self, size: SlideSize) -> Result<()> {
        let (w, h, kind) = size.spec();
        self.set_slide_size(Length::emu(w), Length::emu(h))?;
        if let Some(sz) = self.presentation.sld_sz.as_deref_mut() {
            sz.type_ = kind;
        }
        Ok(())
    }

    /// The slide size as a preset (or [`SlideSize::Custom`]).
    pub fn slide_size_preset(&self) -> SlideSize {
        match self.presentation.sld_sz.as_deref() {
            Some(sz) => SlideSize::from_pml(sz),
            None => SlideSize::Widescreen,
        }
    }

    // ----- presentation properties ------------------------------------------------------

    fn pres_props(&self) -> Result<Option<(PartName, pml::CT_PresentationProperties)>> {
        read_related(
            &self.package,
            Some(&self.part),
            rel_types::PRES_PROPS,
            &pml::elements::PRESENTATION_PR,
        )
    }

    /// The slide show settings.
    pub fn show_settings(&self) -> Result<ShowSettings> {
        Ok(self
            .pres_props()?
            .and_then(|(_, p)| p.show_pr.map(|s| ShowSettings::from_pml(&s)))
            .unwrap_or_default())
    }

    /// Changes the slide show settings (creating `presProps.xml` when missing).
    ///
    /// ```
    /// use openxml_pptx::{Presentation, ShowSettings, ShowType};
    ///
    /// let mut deck = Presentation::new();
    /// deck.set_show_settings(ShowSettings {
    ///     show_type: ShowType::Kiosk { restart_after_ms: Some(300_000) },
    ///     ..ShowSettings::default()
    /// })?;
    /// let settings = deck.show_settings()?;
    /// assert!(settings.loop_until_esc, "kiosk shows loop");
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn set_show_settings(&mut self, settings: ShowSettings) -> Result<()> {
        if let ShowSlides::CustomShow(id) = settings.slides
            && !self.custom_shows().iter().any(|s| s.id == id)
        {
            return Err(Error::NotFound(format!("custom show {id}")));
        }
        let (part, mut props) = match self.pres_props()? {
            Some(found) => found,
            None => {
                let part = self.package.next_part_name("/ppt/presProps{}.xml")?;
                let part = if part.as_str() == "/ppt/presProps1.xml" {
                    PartName::new("/ppt/presProps.xml")?
                } else {
                    part
                };
                self.package
                    .add_relationship(Some(&self.part), rel_types::PRES_PROPS, &part)?;
                (part, pml::CT_PresentationProperties::default())
            }
        };
        settings.apply(props.show_pr.get_or_insert_with(Box::default));
        write_part(
            &mut self.package,
            &part,
            ct::PML_PRES_PROPS,
            &pml::elements::PRESENTATION_PR,
            &props,
        )
    }

    // ----- custom shows ------------------------------------------------------------------

    /// Relationship id of the slide at `index` in the presentation part.
    fn slide_rel_id(&self, index: usize) -> Option<String> {
        let id = self.slides.get(index)?.id;
        self.presentation
            .sld_id_lst
            .as_ref()?
            .sld_id
            .iter()
            .find(|e| e.id == Some(id))?
            .r_id
            .clone()
    }

    /// The custom shows.
    pub fn custom_shows(&self) -> Vec<CustomShow> {
        let position = |rid: &str| {
            let target = self.package.relationship_target(Some(&self.part), rid)?;
            self.slides.iter().position(|s| s.part == target)
        };
        self.presentation
            .cust_show_lst
            .iter()
            .flat_map(|l| &l.cust_show)
            .map(|s| CustomShow {
                id: s.id.unwrap_or(0),
                name: s.name.clone().unwrap_or_default(),
                slides: s
                    .sld_lst
                    .iter()
                    .flat_map(|l| &l.sld)
                    .filter_map(|e| position(e.r_id.as_deref()?))
                    .collect(),
            })
            .collect()
    }

    /// Adds a custom show presenting `slides` (0-based positions, in the
    /// given order; repetitions allowed). Returns its identifier.
    pub fn add_custom_show(&mut self, name: &str, slides: &[usize]) -> Result<u32> {
        let entries = slides
            .iter()
            .map(|&i| {
                self.slide_rel_id(i)
                    .map(|r_id| pml::CT_SlideRelationshipListEntry {
                        r_id: Some(r_id),
                        ..Default::default()
                    })
                    .ok_or_else(|| Error::NotFound(format!("slide {i}")))
            })
            .collect::<Result<Vec<_>>>()?;
        let list = self.presentation.cust_show_lst.get_or_insert_with(Box::default);
        let id = list
            .cust_show
            .iter()
            .filter_map(|s| s.id)
            .map(|i| i + 1)
            .max()
            .unwrap_or(0);
        list.cust_show.push(pml::CT_CustomShow {
            name: Some(name.to_owned()),
            id: Some(id),
            sld_lst: Some(Box::new(pml::CT_SlideRelationshipList {
                sld: entries,
                ..Default::default()
            })),
            ..Default::default()
        });
        self.dirty = true;
        Ok(id)
    }

    /// Removes a custom show. Returns whether it existed.
    pub fn remove_custom_show(&mut self, id: u32) -> bool {
        let Some(list) = self.presentation.cust_show_lst.as_mut() else {
            return false;
        };
        let before = list.cust_show.len();
        list.cust_show.retain(|s| s.id != Some(id));
        let removed = before != list.cust_show.len();
        if list.cust_show.is_empty() {
            self.presentation.cust_show_lst = None;
        }
        self.dirty |= removed;
        removed
    }

    /// Drops custom-show entries whose slide relationship no longer exists.
    pub(crate) fn remove_from_custom_shows(&mut self) {
        let Some(rels) = self.package.relationships(Some(&self.part)) else {
            return;
        };
        let live: Vec<String> = rels.iter().map(|r| r.id.clone()).collect();
        if let Some(list) = self.presentation.cust_show_lst.as_mut() {
            for show in &mut list.cust_show {
                if let Some(sld) = show.sld_lst.as_mut() {
                    sld.sld
                        .retain(|e| e.r_id.as_ref().is_some_and(|r| live.contains(r)));
                }
            }
        }
    }

    // ----- application properties ---------------------------------------------------------

    /// The extended (application) properties of `docProps/app.xml`, if present.
    pub fn app_properties(&self) -> Result<Option<ep::CT_Properties>> {
        Ok(read_related(
            &self.package,
            None,
            rel_types::EXTENDED_PROPERTIES,
            &ep::elements::PROPERTIES,
        )?
        .map(|(_, p)| p))
    }

    /// Edits the extended (application) properties, creating
    /// `docProps/app.xml` when needed. Slide counts are updated whenever slides change.
    ///
    /// ```
    /// use openxml_pptx::Presentation;
    ///
    /// let mut deck = Presentation::new();
    /// deck.edit_app_properties(|p| p.company = Some("ACME".into()))?;
    /// assert_eq!(deck.app_properties()?.unwrap().company.as_deref(), Some("ACME"));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn edit_app_properties(&mut self, f: impl FnOnce(&mut ep::CT_Properties)) -> Result<()> {
        let (part, mut props) = match read_related(
            &self.package,
            None,
            rel_types::EXTENDED_PROPERTIES,
            &ep::elements::PROPERTIES,
        )? {
            Some(found) => found,
            None => {
                let part = PartName::new("/docProps/app.xml")?;
                if self.package.contains(&part) {
                    return Err(Error::InvalidDocument(format!(
                        "{part} exists but is not related to the package"
                    )));
                }
                self.package
                    .add_relationship(None, rel_types::EXTENDED_PROPERTIES, &part)?;
                (part, ep::CT_Properties::default())
            }
        };
        f(&mut props);
        write_part(
            &mut self.package,
            &part,
            ct::EXTENDED_PROPERTIES,
            &ep::elements::PROPERTIES,
            &props,
        )
    }

    // ----- custom properties ---------------------------------------------------------------

    fn custom_part(&self) -> Result<Option<(PartName, cp::CT_Properties)>> {
        read_related(
            &self.package,
            None,
            rel_types::CUSTOM_PROPERTIES,
            &cp::elements::PROPERTIES,
        )
    }

    /// The custom document properties (File > Properties > Custom) as
    /// `(name, value)` pairs; properties of unsupported types are skipped.
    pub fn custom_properties(&self) -> Result<Vec<(String, PropertyValue)>> {
        Ok(self
            .custom_part()?
            .map(|(_, p)| {
                p.property
                    .iter()
                    .filter_map(|prop| {
                        Some((
                            prop.name.clone()?,
                            PropertyValue::from_pml(prop.choice.as_ref()?)?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Sets a custom document property, creating `docProps/custom.xml` when needed.
    ///
    /// ```
    /// use openxml_pptx::{Presentation, PropertyValue};
    ///
    /// let mut deck = Presentation::new();
    /// deck.set_custom_property("Project", PropertyValue::Text("Aurora".into()))?;
    /// deck.set_custom_property("Revision", PropertyValue::Integer(3))?;
    /// assert_eq!(deck.custom_properties()?.len(), 2);
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn set_custom_property(&mut self, name: &str, value: PropertyValue) -> Result<()> {
        if name.is_empty() {
            return Err(Error::InvalidArgument(
                "custom property names cannot be empty".into(),
            ));
        }
        let (part, mut props) = match self.custom_part()? {
            Some(found) => found,
            None => {
                let part = PartName::new("/docProps/custom.xml")?;
                if self.package.contains(&part) {
                    return Err(Error::InvalidDocument(format!(
                        "{part} exists but is not related to the package"
                    )));
                }
                self.package
                    .add_relationship(None, rel_types::CUSTOM_PROPERTIES, &part)?;
                (part, cp::CT_Properties::default())
            }
        };
        match props
            .property
            .iter_mut()
            .find(|p| p.name.as_deref() == Some(name))
        {
            Some(p) => p.choice = Some(value.to_pml()),
            None => {
                let pid = props.property.iter().filter_map(|p| p.pid).max().unwrap_or(1) + 1;
                props.property.push(cp::CT_Property {
                    fmtid: Some(CUSTOM_FMTID.to_owned()),
                    pid: Some(pid.max(2)),
                    name: Some(name.to_owned()),
                    choice: Some(value.to_pml()),
                    ..Default::default()
                });
            }
        }
        write_part(
            &mut self.package,
            &part,
            ct::CUSTOM_PROPERTIES,
            &cp::elements::PROPERTIES,
            &props,
        )
    }

    /// Removes a custom document property. Returns whether it existed.
    pub fn remove_custom_property(&mut self, name: &str) -> Result<bool> {
        let Some((part, mut props)) = self.custom_part()? else {
            return Ok(false);
        };
        let before = props.property.len();
        props.property.retain(|p| p.name.as_deref() != Some(name));
        if before == props.property.len() {
            return Ok(false);
        }
        write_part(
            &mut self.package,
            &part,
            ct::CUSTOM_PROPERTIES,
            &cp::elements::PROPERTIES,
            &props,
        )?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip() {
        for p in [
            SlideSize::Widescreen,
            SlideSize::Standard,
            SlideSize::OnScreen16x9,
            SlideSize::OnScreen16x10,
            SlideSize::Letter,
            SlideSize::A4,
            SlideSize::Slide35mm,
            SlideSize::Overhead,
            SlideSize::Banner,
            SlideSize::Custom(Length::cm(20.0), Length::cm(10.0)),
        ] {
            let (w, h, t) = p.spec();
            let sz = pml::CT_SlideSize {
                cx: Some(w as i32),
                cy: Some(h as i32),
                type_: t,
                ..Default::default()
            };
            assert_eq!(SlideSize::from_pml(&sz), p);
        }
        assert_eq!(SlideSize::A4.dimensions().0, Length::emu(9_906_000));
    }

    #[test]
    fn show_settings_round_trip() {
        for s in [
            ShowSettings::default(),
            ShowSettings {
                show_type: ShowType::Window { scrollbar: false },
                slides: ShowSlides::Range(2, 4),
                loop_until_esc: true,
                narration: false,
                animations: false,
                use_timings: false,
            },
            ShowSettings {
                show_type: ShowType::Kiosk {
                    restart_after_ms: Some(1000),
                },
                slides: ShowSlides::CustomShow(1),
                loop_until_esc: true,
                ..ShowSettings::default()
            },
        ] {
            let mut p = pml::CT_ShowProperties::default();
            s.apply(&mut p);
            assert_eq!(ShowSettings::from_pml(&p), s);
        }
    }

    #[test]
    fn property_values_round_trip() {
        for v in [
            PropertyValue::Text("x".into()),
            PropertyValue::Integer(-4),
            PropertyValue::Number(2.5),
            PropertyValue::Bool(true),
            PropertyValue::Date("2024-01-02T03:04:05Z".into()),
        ] {
            assert_eq!(PropertyValue::from_pml(&v.to_pml()), Some(v));
        }
        assert_eq!(
            PropertyValue::from_pml(&cp::CT_Property_Choice::I2(7)),
            Some(PropertyValue::Integer(7))
        );
        assert_eq!(
            PropertyValue::from_pml(&cp::CT_Property_Choice::Blob(Default::default())),
            None
        );
    }
}
