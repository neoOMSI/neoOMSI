//! The pages! (Backrooms for pages ig?)

use super::*;

mod admin;
mod map;
mod options;
mod vehicle;
mod world;

pub use self::options::{Fmt, OPTION_GROUPS, OptGroup, OptKind, OptRow, OptShow, VOICE_SUB};

pub(super) type DrawFn = fn(&mut Ui, &Renderer, &mut Scene, &Frame, Metrics, f32, f32);

pub(super) struct Page {
    pub nav: &'static str,
    pub draw: DrawFn,
}

pub(super) const PAGES: [Page; 5] = [
    map::PAGE,
    options::PAGE,
    world::PAGE,
    vehicle::PAGE,
    admin::PAGE,
];

pub const PAGE_COUNT: usize = PAGES.len();

pub const OPTIONS_PAGE: usize = 1;

pub const VEHICLE_PAGE: usize = 3;

pub const WORLD_PAGE: usize = 2;

pub const ADMIN_PAGE: usize = 4;

impl Ui {
    pub(in super::super) fn draw_pause_page(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        m: Metrics,
        tab: usize,
        shown: usize,
    ) {
        let Metrics { w, h, u, .. } = m;
        let o = out(self.pause_open);
        self.text
            .rounded(r, scene, [0.0, 0.0, w, h], 0.0, fade([6, 8, 12, 232], o));
        let bar_bottom = self.draw_nav(r, scene, f, m, tab);
        let fade_in = self.page_fade;
        let pt = if fade_in { 1.0 } else { self.page_t };

        if shown != VEHICLE_PAGE {
            self.lab_groups.clear();
            self.lab_actions.clear();
        }
        if shown != WORLD_PAGE && shown != OPTIONS_PAGE && shown != ADMIN_PAGE {
            self.world_groups_rc.clear();
            self.world_sub_rc.clear();
            self.world_rows_rc.clear();
            self.world_drop_rc.clear();
        }
        (PAGES[shown].draw)(self, r, scene, f, m, bar_bottom + 32.0 * u, pt);

        if !fade_in && self.page_t < 1.0 {
            let veil = 0.6 * (1.0 - out(self.page_t / 0.45));
            if veil > 0.0 {
                self.text.rounded(
                    r,
                    scene,
                    [0.0, bar_bottom + m.line, w, h],
                    0.0,
                    fade([6, 8, 12, 255], veil),
                );
            }
        }
        if fade_in {
            let t = self.page_t;
            let cover = out(if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 });
            if cover > 0.0 {
                self.text.rounded(
                    r,
                    scene,
                    [0.0, bar_bottom + m.line, w, h],
                    0.0,
                    fade([6, 8, 12, 255], cover),
                );
            }
        }
    }

    pub(super) fn draw_page_head(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        m: Metrics,
        head: &str,
        note: &str,
        y: f32,
        pt: f32,
    ) -> f32 {
        let Metrics { u, mx, .. } = m;
        let hs = out(pt / 0.6);
        self.text.alpha = hs;
        let hl = self
            .text
            .label(r, scene, &t(head), (34.0 * u) as u32, WHITE);
        hl.place(scene, mx - 40.0 * u * (1.0 - hs), y);
        let ns = out((pt - 0.08) / 0.6);
        self.text.alpha = ns;
        let nl = self
            .text
            .label(r, scene, &t(note), (14.0 * u) as u32, MUTED);
        nl.place(scene, mx - 40.0 * u * (1.0 - ns), y + hl.h as f32);
        self.text.alpha = 1.0;
        y + hl.h as f32 + nl.h as f32 + 18.0 * u
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_indices_match_table() {
        assert_eq!(PAGE_COUNT, PAGES.len());
        assert_eq!(PAGES[OPTIONS_PAGE].nav, "pause.page.options.nav");
        assert!(PAGES[WORLD_PAGE].nav.contains("world"));
        assert!(PAGES[VEHICLE_PAGE].nav.contains("vehicle"));
        assert!(PAGES[ADMIN_PAGE].nav.contains("admin"));
    }

    #[test]
    fn page_indices_are_distinct_and_in_range() {
        let all = [OPTIONS_PAGE, WORLD_PAGE, VEHICLE_PAGE, ADMIN_PAGE];
        for (i, a) in all.iter().enumerate() {
            assert!(*a < PAGE_COUNT);
            assert!(!all[i + 1..].contains(a));
        }
    }

    #[test]
    fn admin_is_last_page() {
        // the nav bar hides the last page when the admin tab is not visible
        assert_eq!(ADMIN_PAGE, PAGE_COUNT - 1);
    }

    #[test]
    fn nav_keys_are_unique() {
        for (i, p) in PAGES.iter().enumerate() {
            assert!(!p.nav.is_empty());
            assert!(PAGES[i + 1..].iter().all(|q| q.nav != p.nav));
        }
    }
}
