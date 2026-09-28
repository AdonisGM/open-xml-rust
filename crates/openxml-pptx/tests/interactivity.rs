//! Hyperlinks, transitions, animations and media: create → save → reopen →
//! assert, with XSD, validator, relationship and identifier checks.

mod common;

use openxml_core::image::tiny_png;
use openxml_opc::known::rel_types;
use openxml_pptx::{
    Animation, CornerDirection, Direction, Effect, EffectClass, EightDirection, LayoutKind, Length, Link,
    MEDIA_REL_TYPE, MediaKind, Orientation, Presentation, ShapeType, SideDirection, Transition,
    TransitionEffect, TransitionSpeed, Trigger,
};
use openxml_schema::pml;

fn cm(v: f64) -> Length {
    Length::cm(v)
}

fn mp4() -> Vec<u8> {
    let mut v = vec![0, 0, 0, 24];
    v.extend_from_slice(b"ftypisom\0\0\x02\0isomiso2mp41");
    v.extend_from_slice(&[0u8; 64]);
    v
}

fn mp3() -> Vec<u8> {
    let mut v = b"ID3\x04\0\0\0\0\0\0".to_vec();
    v.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
    v.extend_from_slice(&[0u8; 64]);
    v
}

#[test]
fn hyperlinks_on_shapes_pictures_and_runs() {
    let mut deck = Presentation::new();
    for _ in 0..3 {
        deck.add_slide(LayoutKind::Blank).unwrap();
    }
    let mut slide = deck.slide_mut(1).unwrap();
    let button = slide
        .add_shape(ShapeType::RoundRect, cm(1.0), cm(1.0), cm(4.0), cm(1.5))
        .id();
    let pic = slide
        .add_picture(&tiny_png(2, 2), cm(6.0), cm(1.0), cm(2.0), None)
        .unwrap();
    let tb = slide
        .add_text_box(cm(1.0), cm(4.0), cm(20.0), cm(2.0), "Visit ")
        .id();
    slide.set_link(button, Some(Link::Slide(2))).unwrap();
    slide
        .set_link(pic, Some(Link::Url("https://example.com/a b".into())))
        .unwrap();
    slide
        .add_link_run(tb, "our site", Link::Url("https://example.com".into()))
        .unwrap();
    slide
        .add_link_run(
            tb,
            " or mail us",
            Link::Email {
                address: "team@example.com".into(),
                subject: Some("Hello there".into()),
            },
        )
        .unwrap();
    let nav = slide.add_text_box(cm(1.0), cm(7.0), cm(20.0), cm(2.0), "").id();
    for (text, link) in [
        ("first", Link::FirstSlide),
        ("last", Link::LastSlide),
        ("next", Link::NextSlide),
        ("previous", Link::PreviousSlide),
        ("end", Link::EndShow),
    ] {
        slide.add_link_run(nav, text, link).unwrap();
    }
    slide.set_run_link(tb, 0, 0, Some(Link::Slide(0))).unwrap();
    assert!(slide.set_run_link(tb, 5, 0, None).is_err());
    assert!(slide.set_link(999, Some(Link::NextSlide)).is_err());
    assert!(slide.set_link(button, Some(Link::Slide(9))).is_err());

    let mut deck = common::round_trip(&mut deck);
    let links = deck.slide_links(1);
    let find = |id: u32, text: Option<&str>| {
        links
            .iter()
            .find(|l| l.shape_id == id && l.text.as_deref() == text)
            .unwrap_or_else(|| panic!("link on {id} {text:?}"))
            .link
            .clone()
    };
    assert_eq!(find(button, None), Link::Slide(2));
    assert_eq!(find(pic, None), Link::Url("https://example.com/a b".into()));
    assert_eq!(find(tb, Some("Visit ")), Link::Slide(0));
    assert_eq!(
        find(tb, Some("our site")),
        Link::Url("https://example.com".into())
    );
    assert_eq!(
        find(tb, Some(" or mail us")),
        Link::Email {
            address: "team@example.com".into(),
            subject: Some("Hello there".into())
        }
    );
    assert_eq!(find(nav, Some("first")), Link::FirstSlide);
    assert_eq!(find(nav, Some("last")), Link::LastSlide);
    assert_eq!(find(nav, Some("next")), Link::NextSlide);
    assert_eq!(find(nav, Some("previous")), Link::PreviousSlide);
    assert_eq!(find(nav, Some("end")), Link::EndShow);
    assert_eq!(links.len(), 10);

    // Removing a link drops its relationship; removing the target slide drops jump links.
    let slide_part = deck.slide(1).unwrap().part_name().clone();
    let hyperlinks = |deck: &Presentation| {
        deck.package()
            .relationships(Some(&slide_part))
            .unwrap()
            .by_type(rel_types::HYPERLINK)
            .count()
    };
    assert_eq!(hyperlinks(&deck), 3);
    deck.slide_mut(1).unwrap().set_link(pic, None).unwrap();
    assert_eq!(hyperlinks(&deck), 2);
    deck.remove_slide(2).unwrap();
    let links = deck.slide_links(1);
    assert!(
        links.iter().all(|l| l.shape_id != button),
        "the jump to the removed slide is gone"
    );
    assert_eq!(deck.slide_links(1).len(), 8);
    let deck = common::round_trip(&mut deck);
    assert_eq!(deck.slide_links(1).len(), 8);
}

#[test]
fn transitions_are_written_read_and_replaced() {
    let mut deck = Presentation::new();
    let effects = [
        TransitionEffect::Fade { through_black: true },
        TransitionEffect::Push(SideDirection::Up),
        TransitionEffect::Wipe(SideDirection::Left),
        TransitionEffect::Split {
            orientation: Orientation::Vertical,
            outward: true,
        },
        TransitionEffect::Cover(EightDirection::Corner(CornerDirection::RightDown)),
        TransitionEffect::Pull(EightDirection::Side(SideDirection::Down)),
        TransitionEffect::Blinds(Orientation::Vertical),
        TransitionEffect::Wheel { spokes: 3 },
        TransitionEffect::Zoom { outward: false },
        TransitionEffect::Strips(CornerDirection::LeftUp),
        TransitionEffect::Dissolve,
        TransitionEffect::Random,
    ];
    for (i, e) in effects.iter().enumerate() {
        let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
        let mut t = Transition::new(*e).speed(if i % 2 == 0 {
            TransitionSpeed::Slow
        } else {
            TransitionSpeed::Medium
        });
        if i % 3 == 0 {
            t = t.advance_on_click(false).advance_after(2500);
        }
        slide.set_transition(Some(t));
    }
    let mut deck = common::round_trip(&mut deck);
    for (i, e) in effects.iter().enumerate() {
        let t = deck.slide(i).unwrap().transition().unwrap();
        assert_eq!(t.effect, Some(*e));
        assert_eq!(t.advance_after_ms, (i % 3 == 0).then_some(2500));
        assert_eq!(t.advance_on_click, i % 3 != 0);
    }
    deck.set_transition_all(Some(Transition::new(TransitionEffect::Cut {
        through_black: false,
    })));
    deck.slide_mut(0).unwrap().set_transition(None);
    let deck = common::round_trip(&mut deck);
    assert_eq!(deck.slide(0).unwrap().transition(), None);
    assert!(
        deck.slides()[1..]
            .iter()
            .all(|s| s.transition().unwrap().effect == Some(TransitionEffect::Cut { through_black: false }))
    );
}

#[test]
fn powerpoint_2010_transitions_are_read_from_their_fallback_and_replaced() {
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Blank).unwrap();
    deck.flush().unwrap();
    // Inject the alternate content PowerPoint writes for a "Prism" transition.
    let part = deck.slide(0).unwrap().part_name().clone();
    let xml = String::from_utf8(deck.package().part(&part).unwrap().data().to_vec()).unwrap();
    let ac = concat!(
        r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">"#,
        r#"<mc:Choice xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" Requires="p14">"#,
        r#"<p:transition spd="slow" p14:dur="1250"><p14:prism/></p:transition></mc:Choice>"#,
        r#"<mc:Fallback><p:transition spd="slow"><p:fade/></p:transition></mc:Fallback></mc:AlternateContent>"#
    );
    let xml = xml.replace("</p:clrMapOvr>", &format!("</p:clrMapOvr>{ac}"));
    deck.package_mut().set_part_data(&part, xml.into_bytes()).unwrap();
    let bytes = deck.package().to_bytes().unwrap();
    let mut deck = Presentation::from_bytes(&bytes).unwrap();
    let t = deck.slide(0).unwrap().transition().unwrap();
    assert_eq!(t.effect, Some(TransitionEffect::Fade { through_black: false }));
    assert_eq!(t.speed, TransitionSpeed::Slow);
    deck.slide_mut(0)
        .unwrap()
        .set_transition(Some(Transition::new(TransitionEffect::Circle)));
    let deck = common::round_trip(&mut deck);
    let slide = deck.slide(0).unwrap();
    assert_eq!(slide.transition().unwrap().effect, Some(TransitionEffect::Circle));
    assert!(
        slide.raw().extra_children.is_empty(),
        "the p14 alternate content is replaced"
    );
}

#[test]
fn animations_follow_powerpoints_structure() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
    let a = slide
        .add_shape(ShapeType::Rect, cm(1.0), cm(1.0), cm(4.0), cm(3.0))
        .id();
    let b = slide
        .add_picture(&tiny_png(4, 4), cm(8.0), cm(1.0), cm(3.0), None)
        .unwrap();
    let c = slide
        .add_text_box(cm(1.0), cm(6.0), cm(10.0), cm(2.0), "Text")
        .id();
    let t = slide
        .add_table(2, 2, cm(12.0), cm(6.0), cm(8.0), cm(2.0))
        .unwrap()
        .id();
    let plan = [
        (a, Animation::new(Effect::Appear)),
        (
            b,
            Animation::new(Effect::FadeIn)
                .trigger(Trigger::WithPrevious)
                .duration(750),
        ),
        (
            c,
            Animation::new(Effect::FlyIn(Direction::Left))
                .trigger(Trigger::AfterPrevious)
                .delay(250),
        ),
        (a, Animation::new(Effect::WipeIn(Direction::Bottom))),
        (
            t,
            Animation::new(Effect::Spin(180.0)).trigger(Trigger::AfterPrevious),
        ),
        (
            b,
            Animation::new(Effect::GrowShrink(1.5)).trigger(Trigger::WithPrevious),
        ),
        (c, Animation::new(Effect::FadeOut)),
        (
            a,
            Animation::new(Effect::FlyOut(Direction::Right))
                .trigger(Trigger::AfterPrevious)
                .duration(1000),
        ),
        (b, Animation::new(Effect::WipeOut(Direction::Top))),
        (
            c,
            Animation::new(Effect::Disappear).trigger(Trigger::AfterPrevious),
        ),
    ];
    for (id, anim) in plan {
        slide.add_animation(id, anim).unwrap();
    }
    assert!(slide.add_animation(999, Animation::new(Effect::Appear)).is_err());

    let deck = common::round_trip(&mut deck);
    let slide = deck.slide(0).unwrap();
    let found = slide.animations();
    assert_eq!(found.len(), plan.len());
    for (info, (id, anim)) in found.iter().zip(plan.iter()) {
        assert_eq!(info.shape_id, *id);
        assert_eq!(info.effect, Some(anim.effect));
        assert_eq!(info.class, anim.effect.class());
        assert_eq!(info.trigger, anim.trigger);
        assert_eq!(info.delay_ms, anim.delay_ms);
        if !matches!(anim.effect, Effect::Appear | Effect::Disappear) {
            assert_eq!(info.duration_ms, anim.duration_ms, "{:?}", anim.effect);
        }
    }
    assert_eq!(found[4].class, EffectClass::Emphasis);
    assert_eq!(slide.animated_shapes(), vec![a, b, c, t]);

    // Structure: tmRoot > mainSeq > 4 click groups (the WithPrevious/AfterPrevious effects join the previous one).
    let timing = slide.raw().timing.as_ref().unwrap();
    let root = &timing.tn_lst.as_ref().unwrap().choice[0];
    let pml::CT_TimeNodeList_Choice::Par(root) = root else {
        panic!()
    };
    let root = root.c_tn.as_ref().unwrap();
    assert_eq!(root.node_type, Some(pml::ST_TLTimeNodeType::TmRoot));
    let pml::CT_TimeNodeList_Choice::Seq(main) = &root.child_tn_lst.as_ref().unwrap().choice[0] else {
        panic!()
    };
    let main = main.c_tn.as_ref().unwrap();
    assert_eq!(main.node_type, Some(pml::ST_TLTimeNodeType::MainSeq));
    assert_eq!(main.child_tn_lst.as_ref().unwrap().choice.len(), 4);
    // Build entries: shapes with text (bldP) and the table (bldGraphic); the picture has none.
    let builds = &timing.bld_lst.as_ref().unwrap().choice;
    let bld_p: Vec<(u32, u32)> = builds
        .iter()
        .filter_map(|b| match b {
            pml::CT_BuildList_Choice::BldP(p) => Some((p.spid.unwrap(), p.grp_id.unwrap())),
            _ => None,
        })
        .collect();
    assert_eq!(bld_p, vec![(a, 0), (c, 0), (a, 1), (c, 1), (a, 2), (c, 2)]);
    assert!(
        builds
            .iter()
            .any(|b| matches!(b, pml::CT_BuildList_Choice::BldGraphic(g) if g.spid == Some(t)))
    );

    // Removing a shape removes its animations; clearing removes the rest.
    let mut deck = deck;
    let mut slide = deck.slide_mut(0).unwrap();
    assert!(slide.remove_shape(c));
    assert_eq!(slide.animations().len(), 7);
    slide.remove_animations(t);
    assert_eq!(slide.animations().len(), 6);
    let deck2 = common::round_trip(&mut deck);
    assert_eq!(deck2.slide(0).unwrap().animations().len(), 6);
    let mut slide = deck.slide_mut(0).unwrap();
    slide.clear_animations();
    assert!(slide.animations().is_empty());
    assert!(slide.raw().timing.is_none());
    common::round_trip(&mut deck);
}

#[test]
fn first_effect_with_previous_starts_with_the_sequence() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
    let a = slide
        .add_shape(ShapeType::Ellipse, cm(1.0), cm(1.0), cm(3.0), cm(3.0))
        .id();
    slide
        .add_animation(a, Animation::new(Effect::FadeIn).trigger(Trigger::AfterPrevious))
        .unwrap();
    let deck = common::round_trip(&mut deck);
    let info = &deck.slide(0).unwrap().animations()[0];
    assert_eq!(info.trigger, Trigger::AfterPrevious);
    let xml = String::from_utf8(
        deck.package()
            .part(deck.slide(0).unwrap().part_name())
            .unwrap()
            .data()
            .to_vec(),
    )
    .unwrap();
    assert!(
        xml.contains(r#"<p:cond evt="onBegin" delay="0"><p:tn val="2"/></p:cond>"#),
        "{xml}"
    );
}

#[test]
fn video_and_audio_are_embedded_with_timing() {
    let mut deck = Presentation::new();
    let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
    let poster = tiny_png(16, 9);
    let video = slide
        .add_video(&mp4(), Some(&poster), cm(2.0), cm(2.0), cm(16.0), cm(9.0))
        .unwrap();
    let audio = slide
        .add_audio(&mp3(), None, cm(20.0), cm(2.0), cm(1.0), cm(1.0))
        .unwrap();
    // The same clip twice is stored once.
    let again = slide
        .add_video(&mp4(), None, cm(2.0), cm(12.0), cm(4.0), cm(3.0))
        .unwrap();
    assert!(
        slide
            .add_video(b"not a video", None, cm(0.0), cm(0.0), cm(1.0), cm(1.0))
            .is_err()
    );
    let mut m4a = mp4();
    m4a.push(1);
    assert!(
        slide
            .add_audio(&m4a, None, cm(0.0), cm(0.0), cm(1.0), cm(1.0))
            .is_ok(),
        "MP4 audio (m4a)"
    );
    slide
        .add_animation(video, Animation::new(Effect::FadeIn))
        .unwrap();

    let mut deck = common::round_trip(&mut deck);
    let media = deck.slide_media(0);
    assert_eq!(media.len(), 4);
    assert_eq!(media[0].shape_id, video);
    assert_eq!(media[0].kind, MediaKind::Video);
    assert_eq!(media[0].content_type.as_deref(), Some("video/mp4"));
    assert_eq!(media[1].shape_id, audio);
    assert_eq!(media[1].kind, MediaKind::Audio);
    assert_eq!(media[1].content_type.as_deref(), Some("audio/mpeg"));
    assert_eq!(media[2].shape_id, again);
    assert_eq!(media[2].part, media[0].part);
    assert_eq!(media[3].content_type.as_deref(), Some("audio/mp4"));
    let part = deck.slide(0).unwrap().part_name().clone();
    let rels = deck.package().relationships(Some(&part)).unwrap();
    assert_eq!(rels.by_type(MEDIA_REL_TYPE).count(), 3);
    assert_eq!(rels.by_type(rel_types::VIDEO).count(), 1);
    assert_eq!(rels.by_type(rel_types::AUDIO).count(), 2);
    let xml = String::from_utf8(deck.package().part(&part).unwrap().data().to_vec()).unwrap();
    assert!(xml.contains(r#"action="ppaction://media""#));
    assert!(xml.contains("{DAA4B4D4-6D71-4841-9C94-3DA8F4ED2E4C}"));
    assert!(xml.contains("<p:video>") && xml.contains("<p:audio>"));
    assert!(xml.contains(r#"nodeType="interactiveSeq""#));
    assert_eq!(
        deck.slide(0).unwrap().animations().len(),
        1,
        "media nodes are not animations"
    );

    // Removing clips removes their timing nodes, relationships and (once unused) parts.
    let mut slide = deck.slide_mut(0).unwrap();
    slide.remove_shape(audio);
    slide.remove_shape(again);
    let deck2 = common::round_trip(&mut deck);
    assert_eq!(deck2.slide_media(0).len(), 2);
    let mut slide = deck.slide_mut(0).unwrap();
    slide.remove_shape(video);
    let deck = common::round_trip(&mut deck);
    let media_parts = deck
        .package()
        .parts()
        .filter(|(n, _)| n.as_str().starts_with("/ppt/media/media"))
        .count();
    assert_eq!(media_parts, 1, "only the m4a clip is left");
    assert!(deck.slide(0).unwrap().animations().is_empty());
}
