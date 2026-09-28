//! Builds a small sample presentation.
//!
//! ```text
//! cargo run -p openxml-pptx --example create_deck -- sample.pptx
//! ```

use openxml_core::image::tiny_png;
use openxml_pptx::{Alignment, FontSize, LayoutKind, Length, Presentation, Result, Rgb};

fn main() -> Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "sample.pptx".into());
    let mut deck = Presentation::new();

    {
        let mut slide = deck.add_slide(LayoutKind::Title)?;
        slide.set_title("openxml-rust")?;
        slide.set_subtitle("PresentationML generated from Rust")?;
        slide.set_notes("Welcome everyone.")?;
    }
    {
        let mut slide = deck.add_slide(LayoutKind::TitleAndContent)?;
        slide.set_title("What is inside")?;
        slide.set_body_levels(&[
            (0, "Typed schema generated from ECMA-376"),
            (1, "Every element and attribute"),
            (0, "Lossless round trips"),
            (1, "Unknown markup keeps its position"),
            (0, "High-level editing API"),
        ])?;
    }
    {
        let mut slide = deck.add_slide(LayoutKind::TitleOnly)?;
        slide.set_title("Numbers")?;
        let mut table = slide.add_table(
            3,
            3,
            Length::cm(2.0),
            Length::cm(4.5),
            Length::cm(29.0),
            Length::cm(4.5),
        )?;
        table.set_values([
            ["Crate", "Purpose", "Tests"],
            ["openxml-xml", "XML I/O", "yes"],
            ["openxml-pptx", "Slides", "yes"],
        ])?;
        slide
            .add_text_box(
                Length::cm(2.0),
                Length::cm(11.0),
                Length::cm(29.0),
                Length::cm(2.0),
                "All parts validate against the XSDs",
            )
            .font_size(FontSize(20.0))
            .italic(true)
            .color(Rgb(0x2F, 0x55, 0x97))
            .align(Alignment::Center);
    }
    {
        let mut slide = deck.add_slide(LayoutKind::Blank)?;
        slide.set_background_color(Rgb(0xF2, 0xF2, 0xF2));
        slide.add_picture(
            &tiny_png(160, 90),
            Length::cm(4.0),
            Length::cm(3.0),
            Length::cm(16.0),
            None,
        )?;
        slide
            .add_text_box(
                Length::cm(21.0),
                Length::cm(6.0),
                Length::cm(10.0),
                Length::cm(3.0),
                "A picture",
            )
            .font_size(FontSize(28.0))
            .bold(true);
    }

    deck.save(&path)?;
    println!("wrote {path} ({} slides)", deck.slide_count());
    Ok(())
}
