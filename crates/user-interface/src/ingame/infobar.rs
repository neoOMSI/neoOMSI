//! Information bar

use super::*;

impl Ui {
    pub(super) fn draw_info_bar(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, s: f32) {
        let Some(info) = f.info.as_ref() else {
            return;
        };

        let cells: Vec<(&str, Option<[u8; 4]>)> = info
            .split('\u{1f}')
            .filter(|c| !c.is_empty())
            .map(|c| match c.strip_prefix('\u{1e}') {
                Some(rest) => {
                    let mut it = rest.chars();
                    let color = match it.next() {
                        Some('L') => [235, 85, 70, 0],
                        Some('E') => [90, 160, 240, 0],
                        _ => [110, 200, 120, 0],
                    };
                    (it.as_str(), Some(color))
                }
                None => (c, None),
            })
            .collect();
        if cells.is_empty() {
            return;
        }
        self.text.flat = true;
        let px = (15.0 * s) as u32;
        let pad = 14.0 * s;
        let h = (34.0 * s).round();
        let max_w = f.width - 24.0 * s;
        let mut widths: Vec<f32> = cells
            .iter()
            .map(|(c, _)| self.text.width(c, px as f32))
            .collect();
        let gaps = pad * 2.0 * cells.len() as f32;
        let total = widths.iter().sum::<f32>() + gaps;
        if total > max_w {
            // too wide: the longest cell gives way
            let over = total - max_w;
            if let Some((i, _)) = widths
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            {
                widths[i] = (widths[i] - over).max(40.0 * s);
            }
        }
        let w = widths.iter().sum::<f32>() + pad * 2.0 * cells.len() as f32;
        let x = ((f.width - w) * 0.5).round();
        let y = (8.0 * s).round();
        let radius = CARD_R * s;
        self.text.rounded(
            r,
            scene,
            [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
            radius + 1.0,
            OPT_LINE,
        );
        self.text
            .rounded(r, scene, [x, y, x + w, y + h], radius, OPT_CARD);
        let line = 1.0_f32.max(s).round();
        let mut cx = x;
        for (i, ((cell, tone), cw)) in cells.iter().zip(widths.iter()).enumerate() {
            if i > 0 {
                self.text.rounded(
                    r,
                    scene,
                    [cx, y + 8.0 * s, cx + line, y + h - 8.0 * s],
                    0.0,
                    OPT_LINE,
                );
            }
            let color = tone.unwrap_or(if i == 0 { txt(ACCENT) } else { WHITE });
            let l = self.text.label(
                r,
                scene,
                &clip_to(&self.text, cell, px as f32, *cw),
                px,
                color,
            );
            l.place(scene, cx + pad, y + (h - l.h as f32) * 0.5);
            cx += cw + pad * 2.0;
        }
        self.text.flat = false;
    }
}
