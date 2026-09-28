//! Read, create and edit PowerPoint presentations (`.pptx`, PresentationML).
//!
//! [`Presentation`] is the entry point. It opens existing files or starts a
//! new 16:9 deck with a slide master, six common layouts and the Office theme,
//! and offers a high-level API for slides, placeholders, text boxes,
//! pictures, tables and speaker notes. Everything is built on the generated
//! schema types of [`openxml_schema::pml`] and [`openxml_schema::dml`], which
//! remain reachable through the `raw()` / `raw_mut()` escape hatches.
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

mod layout;
mod presentation;
mod shape;
mod slide;
mod table;
mod template;
mod text;

pub use layout::{Layout, LayoutKind, LayoutRef};
pub use presentation::Presentation;
pub use shape::{Placeholder, PlaceholderKind, ShapeInfo, ShapeKind, ShapeMut};
pub use slide::{Slide, SlideMut};
pub use table::TableMut;
pub use text::{Alignment, Rgb};

pub use openxml_core::{Error, FontSize, Length, Result};
