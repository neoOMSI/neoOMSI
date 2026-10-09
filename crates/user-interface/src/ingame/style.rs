//! The game menu's colours, sizes and small helpers.

/// How long a fade of the menu takes (a line lit, a switch turned over): a short moment.
pub(super) const FADE_SECS: f32 = 0.15;

pub(super) const PANEL: [u8; 4] = [22, 22, 22, 255];
pub(super) const PANEL_ALT: [u8; 4] = [31, 31, 31, 255];
pub(super) const BORDER: [u8; 4] = [255, 255, 255, 15];
pub(super) const ACCENT: [u8; 4] = [232, 160, 48, 255];
pub(super) const ACCENT_SOFT: [u8; 4] = [232, 160, 48, 34];
pub(super) const DANGER: [u8; 4] = [222, 78, 68, 255];
/// A line under the mouse, and the line chosen.
pub(super) const LIT: [u8; 4] = [255, 255, 255, 16];
pub(super) const CHIP: [u8; 4] = [255, 255, 255, 20];
/// The accent's fill under the mouse, and the ink on it (the launcher's primary button).
pub(super) const ACCENT_HOT: [u8; 4] = [246, 182, 84, 255];
pub(super) const ON_ACCENT: [u8; 4] = [18, 14, 8, 0];
/// Text colours (alpha 0: no outline on the flat panel).
pub(super) const WHITE: [u8; 4] = [236, 236, 236, 0];
pub(super) const SOFT: [u8; 4] = [200, 200, 200, 0];
pub(super) const MUTED: [u8; 4] = [142, 142, 142, 0];
pub(super) const AMBER: [u8; 4] = [255, 200, 110, 0];

/// The card's radius, a line's, and the inset of lines from the card's edge and of their
/// text from the line's edge (all times the scale).
pub(super) const CARD_R: f32 = 8.0;
pub(super) const ROW_R: f32 = 6.0;
pub(super) const PAD: f32 = 12.0;
pub(super) const TEXT_IN: f32 = 16.0;

/// `v` (0 to 1) in eighths.
pub(super) fn quant(v: f32) -> f32 {
    (v.clamp(0.0, 1.0) * 8.0).round() / 8.0
}

/// The colour `t` (0 to 1) of the way from `a` to `b`.
pub(super) fn mix(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]), m(a[3], b[3])]
}

/// `c` with its opacity times `k` (0 to 1).
pub(super) fn fade(c: [u8; 4], k: f32) -> [u8; 4] {
    [
        c[0],
        c[1],
        c[2],
        (c[3] as f32 * k.clamp(0.0, 1.0)).round() as u8,
    ]
}

/// `c` as a text colour (no outline).
pub(super) fn txt(c: [u8; 4]) -> [u8; 4] {
    [c[0], c[1], c[2], 0]
}

/// The distance of the point (`px`, `py`) from the rounded rectangle at (`x0`, `y0`) of
/// `w` x `h` with corners of `rad`: negative inside.
pub(super) fn rr_dist(px: f32, py: f32, x0: f32, y0: f32, w: f32, h: f32, rad: f32) -> f32 {
    let (cx, cy) = (x0 + w * 0.5, y0 + h * 0.5);
    let qx = (px - cx).abs() - (w * 0.5 - rad);
    let qy = (py - cy).abs() - (h * 0.5 - rad);
    (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt() + qx.max(qy).min(0.0) - rad
}