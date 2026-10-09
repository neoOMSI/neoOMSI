//! The pause menu

use super::*;

mod common;
mod dialog;
mod nav;
mod page;
mod place;
mod screen;

use self::common::plain;
pub use self::dialog::Dialog;
pub use self::page::{Fmt, OptGroup, OptKind, OptRow, OptShow, ADMIN_PAGE, OPTIONS_PAGE, OPTION_GROUPS, PAGE_COUNT, VEHICLE_PAGE, WORLD_PAGE};

#[derive(Clone, Debug, Default)]
pub struct WorldRow {
    pub name: String,
    pub kind: char,
    pub value: String,
    pub desc: String,
    pub frac: f32,
    pub tag: String,
    pub meter: Option<f32>,
    /// The meter is a pedal (0 .. 1, filling from the left), not centred (-1 .. 1).
    pub meter_one_sided: bool,
    /// The row's control is pressed now (a controller button): shown highlighted.
    pub lit: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WorldDrop {
    pub k: usize,
    pub labels: Vec<String>,
    pub actions: Vec<String>,
    pub sel: usize,
    pub top: usize,
    pub current: Option<usize>,
    /// Type-to-filter
    pub hay: Vec<String>,
    pub all_labels: Vec<String>,
    pub all_actions: Vec<String>,
    pub filter: String,
}

#[derive(Clone, Debug, Default)]
pub struct WorldGroup {
    pub title: String,
    pub tab: String,
    pub rows: Vec<WorldRow>,
    pub subs: Vec<WorldGroup>,
}

pub const PAUSE_ENTRIES: [&str; 6] = [
    "pause.entry.resume",
    "pause.entry.save",
    "pause.entry.options",
    "pause.entry.world",
    "pause.entry.vehicle",
    "pause.entry.quit",
];

pub(super) fn t(key: &str) -> String {
    ::i18n::translate(key, &[])
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseState {
    pub page: Option<usize>,
    pub sel: usize,
}

impl PauseState {
    pub fn moved(self, n: usize, dir: isize) -> PauseState {
        if n == 0 {
            return self;
        }
        let sel = (self.sel.min(n - 1) as isize + dir).rem_euclid(n as isize) as usize;
        PauseState { sel, ..self }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Metrics {
    pub w: f32,
    pub h: f32,
    pub u: f32,
    pub mx: f32,
    pub line: f32,
}

impl Metrics {
    pub(super) fn new(f: &Frame) -> Metrics {
        let (w, h) = (f.width, f.height);
        let k = (h / (900.0 * f.scale.max(0.5) * f.ui_scale)).clamp(0.55, 1.0);
        let u = f.scale.max(0.5) * f.ui_scale * k;
        Metrics {
            w,
            h,
            u,
            mx: (w * 0.06).max(24.0 * u).round(),
            line: 1.0_f32.max(u).round(),
        }
    }
}

pub(super) fn out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

pub(super) fn fade(c: [u8; 4], k: f32) -> [u8; 4] {
    [c[0], c[1], c[2], (c[3] as f32 * k.clamp(0.0, 1.0)).round() as u8]
}

pub(super) fn mix(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2]), l(a[3], b[3])]
}

pub(super) const SCREEN: usize = usize::MAX;

pub(super) fn inside(rc: [f32; 4], p: (f32, f32)) -> bool {
    p.0 >= rc[0] && p.0 < rc[2] && p.1 >= rc[1] && p.1 < rc[3]
}

impl Ui {

    pub(super) fn pause_reset(&mut self) {
        self.pause_open = 0.0;
        self.page_t = 0.0;
        self.pause_last = SCREEN - 1;
    }

    pub(super) fn draw_pause(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        st: PauseState,
    ) {
        let m = Metrics::new(f);
        let key = st.page.map_or(SCREEN, |t| t.min(PAGE_COUNT - 1));
        if key == SCREEN || self.pause_last >= PAGE_COUNT {
            if self.pause_last != key {
                self.page_fade = false;
                self.pause_last = key;
                self.page_t = 0.0;
            }
        } else if self.pause_last != key {
            self.page_fade = false;
            self.pause_last = key;
            self.page_t = 0.0;
        }
        if self.pause_closing {
            self.pause_open = (self.pause_open - self.anim_dt / 0.2).max(0.0);
        } else {
            self.pause_open = (self.pause_open + self.anim_dt / 0.3).min(1.0);
            self.pause_prev = Some(st);
        }
        self.page_t = (self.page_t + self.anim_dt / 0.36).min(1.0);
        if self.page_fade && key != SCREEN && self.pause_last < PAGE_COUNT && self.page_t >= 0.5 {
            self.pause_last = key;
        }
        match st.page {
            None => {
                self.lab_tabs.clear();
                self.lab_groups.clear();
                self.lab_actions.clear();
                self.draw_pause_screen(r, scene, m, st.sel);
            }
            Some(tab) => {
                self.pause_items.clear();
                let shown = self.pause_last.min(PAGE_COUNT - 1);
                self.draw_pause_page(r, scene, f, m, tab.min(PAGE_COUNT - 1), shown);
            }
        }
        let at = |rs: &[[f32; 4]]| rs.iter().any(|rc| inside(*rc, f.cursor));
        self.hand = match self.dialog.as_ref() {
            Some(Dialog::Place { .. }) => at(&self.place_rects),
            Some(Dialog::Select { drop: Some(_), .. }) => at(&self.dialog_rects) || at(&self.place_rects[..self.place_rects.len().min(4)]),
            Some(Dialog::Loading { .. }) => false,
            Some(_) => at(&self.dialog_rects) || at(&[self.dialog_back_rc]) || at(&self.menu_pane) || at(&self.menu_time) || self.menu_pane_go.as_ref().is_some_and(|g| at(&[*g])),
            None => {
                at(&self.pause_items)
                    || at(&self.lab_tabs)
                    || at(&self.lab_groups)
                    || at(&self.lab_actions)
                    || (self.map_btn_on && at(&[self.map_btn]))
                    || at(&self.world_groups_rc)
                    || at(&self.world_sub_rc)
                    || at(&self.world_rows_rc)
                    || self.world_clear.iter().any(|(_, r)| at(&[*r]))
                    || at(&self.world_drop_rc)
            }
        };
        if self.pause_closing {
            if let Some(d) = self.dialog.take() {
                self.dialog_ghost = Some(d);
            }
        }
        let step = self.anim_dt / 0.15;
        if let Some(d) = self.dialog.take() {
            self.dialog_t = (self.dialog_t + step).min(1.0);
            self.text.alpha = out(self.dialog_t);
            self.draw_dialog(r, scene, f, m, &d);
            self.text.alpha = 1.0;
            self.dialog_ghost = Some(d.clone());
            self.dialog = Some(d);
        } else {
            self.dialog_rects.clear();
            self.dialog_t = (self.dialog_t - step).max(0.0);
            match self.dialog_ghost.take() {
                Some(d) if self.dialog_t > 0.0 => {
                    self.text.alpha = out(self.dialog_t);
                    self.draw_dialog(r, scene, f, m, &d);
                    self.text.alpha = 1.0;
                    self.dialog_rects.clear();
                    self.dialog_ghost = Some(d);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_clamps_and_eases() {
        assert_eq!(out(-1.0), 0.0);
        assert_eq!(out(0.0), 0.0);
        assert_eq!(out(1.0), 1.0);
        assert_eq!(out(2.0), 1.0);
        assert!(out(0.5) > 0.5);
    }

    #[test]
    fn fade_scales_alpha_only() {
        assert_eq!(fade([1, 2, 3, 200], 0.5), [1, 2, 3, 100]);
        assert_eq!(fade([1, 2, 3, 200], 0.0), [1, 2, 3, 0]);
        assert_eq!(fade([1, 2, 3, 200], 5.0), [1, 2, 3, 200]);
        assert_eq!(fade([1, 2, 3, 200], -5.0), [1, 2, 3, 0]);
    }

    #[test]
    fn mix_interpolates_and_clamps() {
        let (a, b) = ([0, 0, 0, 0], [100, 200, 50, 255]);
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, -3.0), a);
        assert_eq!(mix(a, b, 3.0), b);
        assert_eq!(mix(a, b, 0.5), [50, 100, 25, 128]);
    }

    #[test]
    fn inside_is_half_open() {
        let rc = [10.0, 20.0, 30.0, 40.0];
        assert!(inside(rc, (10.0, 20.0)));
        assert!(inside(rc, (29.9, 39.9)));
        assert!(!inside(rc, (30.0, 30.0)));
        assert!(!inside(rc, (20.0, 40.0)));
        assert!(!inside(rc, (9.9, 30.0)));
        assert!(!inside(rc, (20.0, 19.9)));
    }

    #[test]
    fn pause_state_default_is_main_screen() {
        let st = PauseState::default();
        assert_eq!(st.page, None);
        assert_eq!(st.sel, 0);
    }

    #[test]
    fn moved_wraps_both_ways() {
        let st = |sel| PauseState { page: None, sel };
        assert_eq!(st(0).moved(6, 1).sel, 1);
        assert_eq!(st(5).moved(6, 1).sel, 0);
        assert_eq!(st(0).moved(6, -1).sel, 5);
        assert_eq!(st(3).moved(6, -1).sel, 2);
    }

    #[test]
    fn moved_clamps_stale_selection_and_handles_empty() {
        let st = PauseState { page: None, sel: 9 };
        assert_eq!(st.moved(3, 1).sel, 0);
        assert_eq!(st.moved(0, 1), st);
    }

    #[test]
    fn moved_keeps_page() {
        let st = PauseState { page: Some(2), sel: 0 };
        assert_eq!(st.moved(4, 1).page, Some(2));
    }

    #[test]
    fn screen_marker_is_not_a_page() {
        assert!(SCREEN >= PAGE_COUNT);
        assert!(SCREEN - 1 >= PAGE_COUNT);
    }

    #[test]
    fn pause_entries_are_unique_keys() {
        for (i, a) in PAUSE_ENTRIES.iter().enumerate() {
            assert!(a.starts_with("pause.entry."));
            assert!(!PAUSE_ENTRIES[i + 1..].contains(a));
        }
    }
}
