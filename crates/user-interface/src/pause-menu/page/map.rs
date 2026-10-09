//! The map page: Navigator and more

use super::*;

pub(super) const PAGE: Page = Page {
    nav: "pause.page.map.nav",
    draw: Ui::draw_map_page,
};

impl Ui {
    pub(super) fn draw_map_page(
        &mut self,
        _r: &Renderer,
        scene: &mut Scene,
        _f: &Frame,
        m: Metrics,
        top: f32,
        _pt: f32,
    ) {
        let Metrics { w, h, u, line, .. } = m;
        let rc = [0.0, (top - 32.0 * u).round() + line, w, h];
        self.map_rect = rc;
        if let Some(tex) = self.map_picture {
            scene.overlays.push((tex, rc));
        }
    }
}