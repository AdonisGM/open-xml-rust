//! Read, create and edit PowerPoint presentations (`.pptx`, PresentationML).
//!
//! [`Presentation`] is the entry point. It opens existing files or starts a
//! new 16:9 deck with a slide master, six common layouts and the Office theme,
//! and offers a high-level API for slides, placeholders, text boxes,
//! pictures, tables and speaker notes. Everything is built on the generated
//! schema types of [`openxml_schema::pml`] and [`openxml_schema::dml`], which
//! remain reachable through the `raw()` / `raw_mut()` escape hatches.
//!
//! What the API covers:
//!
//! - **Shapes** ([`SlideMut::add_shape`], [`ShapeMut`]): preset geometries and
//!   adjust values, freeforms, solid/gradient/pattern/picture fills, outlines
//!   with dashes and arrowheads, shadows, rotation and flips, text frames
//!   (anchoring, insets, autofit, direction, columns); connectors glued to
//!   shapes ([`SlideMut::connect_shapes`]); groups ([`SlideMut::group`]).
//! - **Text** ([`ParagraphMut`], [`RunMut`]): bullets and numbering, levels,
//!   indents, spacing, fonts per script, highlight, baseline, shadows.
//! - **Pictures** ([`PictureMut`]): cropping, transparency, borders, effects,
//!   replacement, picture placeholders, alternative text for every graphic.
//! - **Tables** ([`TableMut`]): merged cells, cell fills, borders, margins,
//!   anchoring, styles and style options, row and column editing.
//! - **Interactivity**: hyperlinks ([`Link`]), transitions ([`Transition`]),
//!   animations ([`Animation`]) and embedded audio/video ([`SlideMut::add_video`]).
//! - **Slides**: duplication, import from other presentations, hiding,
//!   formatted notes, legacy comments; sections are kept consistent.
//! - **Masters**: theme colours and fonts, backgrounds, custom layouts
//!   ([`Presentation::add_layout`]), footers ([`Presentation::apply_header_footer`]).
//! - **Presentation**: slide size presets, show settings, custom shows,
//!   core, application and custom document properties.
//!
//! Charts are not generated; [`SlideMut::add_graphic_frame`] hosts any
//! `a:graphicData` (see [`chart_graphic_data`]) so they can be added on top.
//!
//! ```
//! use openxml_core::{FontSize, Length};
//! use openxml_pptx::{LayoutKind, Presentation, Rgb};
//!
//! let mut deck = Presentation::new();
//! {
//!     let mut title = deck.add_slide(LayoutKind::Title)?;
//!     title.set_title("Project Aurora")?;
//!     title.set_subtitle("Kick-off meeting")?;
//! }
//! {
//!     let mut slide = deck.add_slide(LayoutKind::TitleOnly)?;
//!     slide.set_title("Budget")?;
//!     let mut table = slide.add_table(2, 2, Length::cm(2.0), Length::cm(5.0), Length::cm(20.0), Length::cm(3.0))?;
//!     table.set_values([["Item", "Cost"], ["Servers", "12 000"]])?;
//!     slide
//!         .add_text_box(Length::cm(2.0), Length::cm(10.0), Length::cm(20.0), Length::cm(2.0), "Draft")
//!         .font_size(FontSize(28.0))
//!         .color(Rgb(0xC0, 0, 0));
//!     slide.set_notes("Mention the hardware discount.")?;
//! }
//! let bytes = deck.to_bytes()?;
//!
//! let reopened = openxml_pptx::Presentation::from_bytes(&bytes)?;
//! let budget = reopened.slide(1).unwrap();
//! assert_eq!(budget.title().as_deref(), Some("Budget"));
//! assert!(budget.text().contains("Servers\t12 000"));
//! assert_eq!(budget.notes_text().as_deref(), Some("Mention the hardware discount."));
//! # Ok::<(), openxml_core::Error>(())
//! ```

#![warn(missing_docs)]

mod animation;
mod clone;
mod comments;
mod connector;
mod drawing;
mod format;
mod graphic_frame;
mod group;
mod hyperlink;
mod layout;
mod media;
mod paragraph;
mod picture;
mod presentation;
mod properties;
mod sections;
mod shape;
mod slide;
mod table;
mod template;
mod text;
mod theme;
mod transition;
mod util;

pub use animation::{Animation, AnimationInfo, Direction, Effect, EffectClass, Trigger};
pub use comments::{Comment, CommentAuthor};
pub use connector::{ConnectorKind, ConnectorMut, Side};
pub use drawing::{
    ArrowHead, ArrowKind, ArrowSize, Color, Fill, Gradient, Line, LineDash, PatternType, SchemeColor, Shadow,
    ShapeType,
};
pub use format::{Autofit, Freeform, PathCommand, TextAnchor, TextDirection};
pub use graphic_frame::chart_graphic_data;
pub use hyperlink::{Link, LinkInfo};
pub use layout::{Layout, LayoutKind, LayoutMut, LayoutRef};
pub use media::{MEDIA_REL_TYPE, MediaFormat, MediaInfo, MediaKind, sniff_media};
pub use paragraph::{AutoNumberScheme, Bullet, ParagraphMut, RunMut, Spacing};
pub use picture::{Crop, PictureMut};
pub use presentation::Presentation;
pub use properties::{CustomShow, PropertyValue, ShowSettings, ShowSlides, ShowType, SlideSize};
pub use sections::Section;
pub use shape::{Placeholder, PlaceholderKind, ShapeInfo, ShapeKind, ShapeMut};
pub use slide::{Slide, SlideMut};
pub use table::{CellBorder, TableFlags, TableMut, TableStyle};
pub use text::{Alignment, Rgb};
pub use theme::{DateField, FontSet, HeaderFooter, ThemeColors, ThemeFonts};
pub use transition::{
    CornerDirection, EightDirection, Orientation, SideDirection, Transition, TransitionEffect,
    TransitionSpeed,
};

pub use openxml_core::{Error, FontSize, Length, Result};
