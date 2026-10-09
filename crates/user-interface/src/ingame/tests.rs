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
