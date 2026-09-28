//! Builds a sample presentation that exercises most of the API: shapes,
//! connectors, groups, formatted text, tables, pictures, hyperlinks,
//! transitions, animations, media, comments, themes, layouts and footers.
//!
//! ```text
//! cargo run -p openxml-pptx --example create_deck -- sample.pptx
//! ```

use openxml_core::image::tiny_png;
use openxml_pptx::{
    Alignment, Animation, ArrowHead, ArrowKind, AutoNumberScheme, Autofit, Bullet, CellBorder, ConnectorKind,
    Crop, DateField, Direction, Effect, Fill, FontSize, Gradient, HeaderFooter, LayoutKind, Length, Line,
    LineDash, Link, PlaceholderKind, Presentation, PropertyValue, Result, Rgb, SchemeColor, Shadow,
    ShapeType, ShowSettings, Side, SideDirection, Spacing, TableFlags, TableStyle, TextAnchor, TextDirection,
    Transition, TransitionEffect, TransitionSpeed, Trigger,
};

fn cm(v: f64) -> Length {
    Length::cm(v)
}

fn main() -> Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "sample.pptx".into());
    let mut deck = Presentation::new();

    // Theme and a custom layout with a picture placeholder.
    let mut colors = deck.theme_colors()?;
    colors.accent1 = Rgb(0x1F, 0x4E, 0x79);
    colors.accent2 = Rgb(0xE0, 0x7A, 0x1F);
    deck.set_theme_colors(&colors, Some("openxml-rust"))?;
    {
        let mut layout = deck.add_layout("Picture with Title", LayoutKind::TitleOnly)?;
        layout.add_placeholder(PlaceholderKind::Picture, cm(2.0), cm(4.2), cm(20.0), cm(11.25));
    }

    {
        let mut slide = deck.add_slide(LayoutKind::Title)?;
        slide.set_title("openxml-rust")?;
        slide.set_subtitle("PresentationML generated from Rust")?;
        slide.set_notes("Welcome everyone.")?;
        slide.set_transition(Some(Transition::new(TransitionEffect::Fade {
            through_black: true,
        })));
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
        slide.set_transition(Some(
            Transition::new(TransitionEffect::Push(SideDirection::Up)).speed(TransitionSpeed::Medium),
        ));
    }
    {
        // Shapes, connectors and a group, with animations.
        let mut slide = deck.add_slide(LayoutKind::TitleOnly)?;
        slide.set_title("Shapes and animations")?;
        let start = {
            let mut s = slide.add_shape(ShapeType::RoundRect, cm(2.0), cm(5.0), cm(7.0), cm(3.5));
            s.set_text("Plan");
            s.set_shadow(Some(Shadow::default())).set_adjust("adj", 25000);
            s.id()
        };
        let end = {
            let mut s = slide.add_shape(ShapeType::Ellipse, cm(22.0), cm(11.0), cm(7.0), cm(3.5));
            s.set_text("Ship");
            s.set_fill(Fill::Gradient(Gradient::linear(
                SchemeColor::Accent2,
                Rgb(0xFF, 0xD9, 0x66),
                90.0,
            )));
            s.id()
        };
        let star = {
            let mut s = slide.add_shape(ShapeType::Star5, cm(13.0), cm(5.5), cm(4.0), cm(4.0));
            s.set_fill(Fill::solid(Rgb(0xFF, 0xC0, 0x00)))
                .set_line(Line::solid(Rgb(0x80, 0x60, 0x00), Length::pt(1.5)).dash(LineDash::Dash))
                .set_rotation(12.0);
            s.id()
        };
        slide
            .connect_shapes(ConnectorKind::Elbow, start, Side::Right, end, Side::Left)?
            .set_line(
                Line::solid(Rgb(0x40, 0x40, 0x40), Length::pt(2.0)).tail(ArrowHead::new(ArrowKind::Triangle)),
            );
        let badge = slide
            .add_shape(ShapeType::Rect, cm(24.0), cm(4.5), cm(4.0), cm(1.2))
            .id();
        let label = slide
            .add_text_box(cm(24.0), cm(5.8), cm(4.0), cm(1.0), "beta")
            .id();
        slide.group(&[badge, label])?;
        slide.add_animation(start, Animation::new(Effect::FadeIn))?;
        slide.add_animation(
            star,
            Animation::new(Effect::Spin(360.0))
                .trigger(Trigger::AfterPrevious)
                .duration(1500),
        )?;
        slide.add_animation(
            end,
            Animation::new(Effect::FlyIn(Direction::Right)).trigger(Trigger::OnClick),
        )?;
    }
    {
        // Formatted text.
        let mut slide = deck.add_slide(LayoutKind::TitleOnly)?;
        slide.set_title("Text")?;
        let mut tb = slide.add_text_box(cm(2.0), cm(4.5), cm(14.0), cm(12.0), "");
        tb.set_autofit(Autofit::SHRINK).set_text_anchor(TextAnchor::Top);
        tb.add_paragraph("Numbered")
            .set_bullet(Bullet::Numbered {
                scheme: AutoNumberScheme::ArabicPeriod,
                start_at: 1,
            })
            .set_space_after(Spacing::Points(6.0));
        tb.add_paragraph("Also numbered")
            .set_bullet(Bullet::numbered())
            .set_line_spacing(Spacing::Lines(1.2));
        let mut p = tb.add_paragraph("");
        p.set_level(1).set_bullet(Bullet::Char {
            char: '–',
            font: None,
        });
        p.add_run("Bold ").bold(true).size(FontSize(24.0));
        p.add_run("accent").color(SchemeColor::Accent2);
        p.add_run(" tiếng Việt")
            .highlight(Rgb(0xFF, 0xFF, 0x99))
            .language("vi-VN");
        let mut v = slide.add_shape(ShapeType::Rect, cm(18.0), cm(4.5), cm(3.0), cm(12.0));
        v.set_text("Vertical text");
        v.set_text_direction(TextDirection::Vertical270);
        let mut cols = slide.add_text_box(
            cm(22.0),
            cm(4.5),
            cm(10.0),
            cm(12.0),
            "Two columns of text flow here, left to right and top to bottom, as in a newspaper.",
        );
        cols.set_columns(2, cm(0.5)).align(Alignment::Justify);
    }
    {
        let mut slide = deck.add_slide(LayoutKind::TitleOnly)?;
        slide.set_title("Numbers")?;
        let mut table = slide.add_table(4, 3, cm(2.0), cm(4.5), cm(29.0), cm(6.0))?;
        table.set_values([
            ["Crate", "Purpose", "Tests"],
            ["openxml-xml", "XML I/O", "yes"],
            ["openxml-pptx", "Slides", "yes"],
            ["Total", "", "2 crates"],
        ])?;
        table.merge_cells(3, 0, 3, 1)?;
        table
            .set_style(TableStyle::MediumStyle2Accent1)
            .set_flags(TableFlags {
                first_row: true,
                last_row: true,
                banded_rows: true,
                ..TableFlags::default()
            });
        table.set_cell_fill(2, 2, Fill::solid(Rgb(0xE2, 0xEF, 0xDA)))?;
        table.set_cell_border(
            2,
            2,
            CellBorder::Bottom,
            Line::solid(Rgb(0x38, 0x76, 0x1D), Length::pt(2.0)),
        )?;
        table.set_cell_anchor(0, 0, TextAnchor::Middle)?;
        table.set_alt_text(Some("Crates"), "The crates of the workspace and their purpose");
        let id = slide
            .add_text_box(
                cm(2.0),
                cm(11.5),
                cm(29.0),
                cm(2.0),
                "All parts validate against the XSDs — ",
            )
            .font_size(FontSize(20.0))
            .italic(true)
            .color(Rgb(0x2F, 0x55, 0x97))
            .align(Alignment::Center)
            .id();
        slide.add_link_run(
            id,
            "ECMA-376",
            Link::Url("https://ecma-international.org/publications-and-standards/standards/ecma-376/".into()),
        )?;
        slide.add_comment(
            "openxml-rust",
            "OX",
            "Numbers from the test suite",
            cm(2.0),
            cm(4.5),
        )?;
    }
    {
        let mut slide = deck.add_slide("Picture with Title")?;
        slide.set_title("Pictures")?;
        let id = slide.fill_picture_placeholder(&tiny_png(160, 90))?;
        slide
            .picture_mut(id)
            .expect("just filled")
            .set_alt_text(None, "A grey test image");
        let pic = slide.add_picture(&tiny_png(90, 90), cm(23.5), cm(4.2), cm(8.0), None)?;
        slide
            .picture_mut(pic)
            .expect("just added")
            .set_crop(Crop {
                left: 0.1,
                top: 0.1,
                right: 0.1,
                bottom: 0.1,
            })
            .set_transparency(0.3)
            .set_line(Line::solid(Rgb::WHITE, Length::pt(3.0)))
            .set_shadow(Some(Shadow::default()))
            .set_alt_text(Some("Thumbnail"), "A cropped, semi-transparent thumbnail");
        slide.set_link(pic, Some(Link::FirstSlide))?;
    }
    {
        let mut slide = deck.add_slide(LayoutKind::TitleOnly)?;
        slide.set_title("Media")?;
        // A minimal MP4 header: enough to show the embedding structure.
        let mut mp4 = vec![0, 0, 0, 24];
        mp4.extend_from_slice(b"ftypisom\0\0\x02\0isomiso2mp41");
        slide.add_video(
            &mp4,
            Some(&tiny_png(160, 90)),
            cm(6.0),
            cm(4.5),
            cm(21.0),
            cm(11.8),
        )?;
        slide.set_background(Fill::solid(Rgb(0x20, 0x20, 0x20)));
    }

    deck.duplicate_slide(1)?;
    deck.slide_mut(2).expect("duplicated").set_hidden(true);
    deck.apply_header_footer(&HeaderFooter {
        footer: Some("openxml-rust sample".into()),
        slide_number: true,
        date: Some(DateField::Automatic),
        skip_title_slides: true,
    })?;
    deck.add_custom_show("Short", &[0, 3, 4])?;
    deck.set_show_settings(ShowSettings {
        loop_until_esc: true,
        ..ShowSettings::default()
    })?;
    deck.set_custom_property("Generator", PropertyValue::Text("openxml-pptx example".into()))?;

    deck.save(&path)?;
    println!("wrote {path} ({} slides)", deck.slide_count());
    Ok(())
}
