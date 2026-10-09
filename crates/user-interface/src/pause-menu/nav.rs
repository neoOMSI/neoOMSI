//! The nav bar on top of the pages.

use super::*;

fn underline(lefts: &[f32], widths: &[f32], pos: f32) -> (f32, f32) {
    let last = lefts.len() - 1;
    let pos = pos.clamp(0.0, last as f32);
    let i0 = (pos.floor() as usize).min(last);
    let i1 = (i0 + 1).min(last);
    let fr = pos - i0 as f32;
    let s = fr * fr * (3.0 - 2.0 * fr);
    (
        lefts[i0] + (lefts[i1] - lefts[i0]) * s,
        widths[i0] + (widths[i1] - widths[i0]) * s,
    )
}

impl Ui {
    pub(super) fn draw_nav(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        m: Metrics,
        tab: usize,
    ) -> f32 {
        let Metrics { w, u, mx, line, .. } = m;
        self.lab_tabs.clear();
        let bar_h = (56.0 * u).round();
        self.text.rounded(r, scene, [0.0, 0.0, w, bar_h], 0.0, [0, 0, 0, 200]);
        self.text
            .rounded(r, scene, [0.0, bar_h, w, bar_h + line], 0.0, BORDER);
        self.ensure_logo();
        if let Some((tex, iw, ih)) = self.logo_at(r, scene, (bar_h * 0.46).round()) {
            let (lw, lh) = (iw as f32, ih as f32);
            let top = ((bar_h - lh) * 0.5).round();
            scene.overlays.push((tex, [mx, top, mx + lw, top + lh]));
        } else {
            let brand = self.text.label(r, scene, "neoOMSI", (15.0 * u) as u32, MUTED);
            brand.place(scene, mx, (bar_h - brand.h as f32) * 0.5);
        }

        let tpx = (17.0 * u) as u32;
        let gap = 6.0 * u;
        let pages = &page::PAGES[..if self.admin_visible { page::PAGES.len() } else { page::PAGES.len() - 1 }];
        let widths: Vec<f32> = pages
            .iter()
            .map(|p| self.text.width(&t(p.nav), tpx as f32) + 40.0 * u)
            .collect();
        let total = widths.iter().sum::<f32>() + gap * (widths.len() - 1) as f32;
        let mut x = ((w - total) * 0.5).round();
        for (label, bx) in [("Q", x - 40.0 * u), ("E", x + total + 12.0 * u)] {
            let l = self.text.label(r, scene, label, (13.0 * u) as u32, WHITE);
            let (bw, bh) = (28.0 * u, 24.0 * u);
            let by = (bar_h - bh) * 0.5;
            self.text
                .rounded(r, scene, [bx, by, bx + bw, by + bh], 4.0 * u, [255, 255, 255, 56]);
            l.place(scene, bx + (bw - l.w as f32) * 0.5, by + (bh - l.h as f32) * 0.5);
        }
        let mut lefts = Vec::with_capacity(widths.len());
        let mut lx = x;
        for wd in &widths {
            lefts.push(lx);
            lx += wd + gap;
        }
        let pos = self.ease((201, "pos", 0), tab as f32, 9.0);
        for (i, p) in pages.iter().enumerate() {
            let rc = [x, 0.0, x + widths[i], bar_h];
            let hot = inside(rc, f.cursor);
            let hv = self.ease((201, "hover", i), if hot && i != tab { 1.0 } else { 0.0 }, 8.0);
            let tv = self.easeq((201, "tab", i), if i == tab { 1.0 } else { 0.0 }, 7.0);
            if hv > 0.0 {
                self.text.rounded(r, scene, rc, 0.0, fade(LIT, hv));
            }
            let name = t(p.nav);
            let l = self.text.label(
                r,
                scene,
                &name,
                tpx,
                mix(if hot { SOFT } else { MUTED }, WHITE, tv),
            );
            l.place(scene, x + (widths[i] - l.w as f32) * 0.5, (bar_h - l.h as f32) * 0.5);
            self.lab_tabs.push(rc);
            x += widths[i] + gap;
        }
        // one underline that slides from tab to tab
        let (ul, uw) = underline(&lefts, &widths, pos);
        self.text
            .rounded(r, scene, [ul, bar_h - 3.0 * u, ul + uw, bar_h], 0.0, ACCENT);
        bar_h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: [f32; 3] = [0.0, 100.0, 220.0];
    const W: [f32; 3] = [90.0, 110.0, 70.0];

    #[test]
    fn underline_sits_on_tab_at_integer_pos() {
        for i in 0..3 {
            assert_eq!(underline(&L, &W, i as f32), (L[i], W[i]));
        }
    }

    #[test]
    fn underline_halfway_is_midpoint() {
        assert_eq!(underline(&L, &W, 0.5), (50.0, 100.0));
        assert_eq!(underline(&L, &W, 1.5), (160.0, 90.0));
    }

    #[test]
    fn underline_clamps_out_of_range() {
        assert_eq!(underline(&L, &W, -3.0), (L[0], W[0]));
        assert_eq!(underline(&L, &W, 9.0), (L[2], W[2]));
    }

    #[test]
    fn underline_single_tab() {
        assert_eq!(underline(&[5.0], &[7.0], 0.7), (5.0, 7.0));
    }

    #[test]
    fn underline_is_monotonic_between_tabs() {
        let mut prev = underline(&L, &W, 0.0).0;
        for k in 1..=10 {
            let x = underline(&L, &W, k as f32 / 10.0).0;
            assert!(x >= prev);
            prev = x;
        }
    }
}
