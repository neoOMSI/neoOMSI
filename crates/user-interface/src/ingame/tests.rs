//! Tests of the in-game interface.

use super::*;

#[test]
fn text_renders_with_an_outline() {
    let f = FontVec::try_from_vec(ROBOTO.to_vec()).unwrap();
    let img = render_text(&f, "Savva: hi", 20.0, [255, 255, 255, 220]);
    assert!(img.width > 40 && img.height > 14);
    let px: Vec<&[u8]> = img.rgba.chunks(4).collect();
    assert!(px.iter().any(|p| p[3] > 200 && p[0] > 200));
    assert!(px.iter().any(|p| p[3] > 100 && p[0] < 40));
}

/// The Esc menu in Chinese, Korean and Thai: the system's fonts, not boxes.
#[test]
fn scripts_roboto_lacks_come_from_the_system() {
    let f = FontVec::try_from_vec(ROBOTO.to_vec()).unwrap();
    for t in ["继续", "繼續", "계속", "ดำเนินการต่อ"] {
        if t.chars()
            .next()
            .and_then(crate::text::fallback_font)
            .is_none()
        {
            continue;
        }
        for c in t.chars() {
            let g = font_for(&f, c);
            assert!(!std::ptr::eq(g, &f) && g.glyph_id(c).0 != 0, "{t}: {c}");
        }
        let img = render_text(&f, t, 20.0, [255, 255, 255, 220]);
        let ink = img
            .rgba
            .chunks(4)
            .filter(|p| p[3] > 128 && p[0] > 128)
            .count();
        assert!(ink > 30, "{t}: {ink}");
    }
}

/// Laid out for 1080p: a lower window as it always was, a taller one in proportion.
#[test]
fn the_interface_grows_with_tall_windows() {
    assert_eq!(size_factor(900.0, 1.0, 1.0, true), 1.0);
    assert_eq!(size_factor(1080.0, 1.0, 1.0, true), 1.0);
    assert_eq!(size_factor(2160.0, 1.0, 1.0, true), 2.0);
    assert_eq!(size_factor(4320.0, 1.0, 1.0, true), 2.0);
    assert!((1.5 * size_factor(2160.0, 1.5, 1.0, true) - 2.0).abs() < 1e-5);
    assert_eq!(size_factor(1080.0, 1.0, 1.5, true), 1.5);
    assert_eq!(size_factor(2160.0, 1.0, 0.5, true), 1.0);
    assert_eq!(size_factor(2160.0, 1.0, 1.0, false), 1.0);
    assert_eq!(size_factor(2160.0, 1.0, 1.25, false), 1.25);
}

/// A thinned panel's light texts get an outline; dark ones - the highlighted menu line's,
/// on the solid accent - do not (it came out as a smear round them, like a shadow).
#[test]
fn panel_texts_get_an_outline_only_where_it_helps() {
    assert_eq!(outline_for([235, 235, 235, 0], 1.0), 0);
    assert!(outline_for([235, 235, 235, 0], 0.47) > 100);
    assert!(outline_for([140, 140, 140, 0], 0.47) > 100);
    assert_eq!(outline_for([20, 20, 20, 0], 0.47), 0);
    assert_eq!(outline_for([255, 255, 255, 235], 0.47), 235);
}

/// The opacity's default is the design; below it the backgrounds fade, never quite away.
#[test]
fn backgrounds_follow_the_opacity_setting() {
    assert_eq!(backdrop(0.85), 1.0);
    assert!((backdrop(0.425) - 0.5).abs() < 1e-6);
    assert_eq!(backdrop(0.2), 0.3);
    assert!(backdrop(1.0) > 1.0);
}

#[test]
fn chat_filter_stars_out_swearing() {
    assert_ne!(
        filter_chat("you are a fucking idiot"),
        "you are a fucking idiot"
    );
    assert_eq!(
        filter_chat("next stop Rathaus Spandau"),
        "next stop Rathaus Spandau"
    );
}

#[test]
fn a_path_wider_than_the_line_is_broken_after_a_folder() {
    let tc = TextCache::new().unwrap();
    let path = "Vehicles/MAN_NL202/Texture/Repaints/Very_Long_Repaint_Name_Here/front.dds";
    let width = tc.width_raw("Vehicles/MAN_NL202/Texture/", 15.0) + 4.0;
    let lines = wrap(&tc, &format!("Missing: {path}"), 15.0, width);
    assert!(lines.iter().all(|line| tc.width_raw(line, 15.0) <= width), "{lines:?}");
    assert_eq!(lines.concat().replace("Missing:", ""), path, "nothing is lost");
    assert!(lines.iter().any(|line| line.ends_with("Texture/")), "{lines:?}");
}

/// #141: the timetable and the info bar fade with the opacity setting, their texts outlined.
#[test]
fn timetable_and_info_bar_follow_the_opacity_setting() {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::NOOP;
    desc.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let r = pollster::block_on(Renderer::new_with(
        &wgpu::Instance::new(desc),
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ::render::RenderOptions {
            msaa: 1,
            ssao: false,
            no_enhanced: true,
            shadow_size: 512,
            ..Default::default()
        },
    ))
    .unwrap();
    let cards = |setting: f32| {
        let mut ui = Ui::new().unwrap();
        let mut scene = r.new_scene();
        let mut f = Frame::bare(1.0, 1.0, backdrop(setting), 1920.0, 1080.0, (0.0, 0.0));
        f.timetable = Some((
            "2 > 2 SALERIN".into(),
            (0..6)
                .map(|k| (format!("Stop {k}"), format!("11:4{k}"), (k == 1) as u8))
                .collect(),
        ));
        f.info = Some("12:00:53\u{1f}0 km/h\u{1f}Next stop: Beaulieu".into());
        ui.draw(&r, &mut scene, &f, 0.016);
        let keys = ui.text.labels.keys();
        let card = keys
            .clone()
            .filter(|k| k.0.starts_with('\0') && k.2[..3] == OPT_CARD[..3])
            .map(|k| k.2[3])
            .collect::<Vec<u8>>();
        let outline = keys.filter(|k| k.0 == "Stop 2").map(|k| k.2[3]).collect::<Vec<u8>>();
        (card, outline)
    };
    let (thin, outline) = cards(0.2);
    assert!(!thin.is_empty());
    assert!(thin.iter().all(|&a| a < 100), "{thin:?}");
    assert!(outline.len() == 1 && outline[0] > 100, "{outline:?}");
    let (full, outline) = cards(1.0);
    assert!(!full.is_empty() && full.iter().all(|&a| a == 255), "{full:?}");
    assert_eq!(outline, [0]);
}

/// A stripe on a see-through card lightens it as on the solid one, not darker.
#[test]
fn stripes_on_a_see_through_card_lighten_it() {
    assert_eq!(on_card(OPT_ROW, 1.0), OPT_ROW);
    for c in [OPT_ROW, OPT_GROUP_ON, OPT_LINE] {
        let v = on_card(c, 0.3);
        assert_eq!(v[..3], [255, 255, 255]);
        let over = mix(OPT_CARD, [255, 255, 255, 255], v[3] as f32 / 255.0);
        for i in 0..3 {
            assert!((over[i] as i32 - c[i] as i32).abs() <= 2, "{c:?}: {over:?}");
        }
    }
}

/// A card: border at the edge, fill inside, empty corners.
#[test]
fn a_card_is_its_fill_inside_a_one_pixel_border() {
    let (fill, border) = ([18, 18, 20, 77], [40, 40, 44, 77]);
    let px = super::shapes::card_pixels(40, 30, 8.0, fill, border);
    let at = |x: usize, y: usize| &px[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4];
    assert_eq!(at(20, 15), fill);
    assert_eq!(at(20, 0), border);
    assert_eq!(at(0, 15), border);
    assert_eq!(at(0, 0)[3], 0);
}
