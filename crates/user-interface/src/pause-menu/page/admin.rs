//! Administration of the session

use super::*;

pub(super) const PAGE: Page = Page {
    nav: "pause.page.admin.nav",
    draw: Ui::draw_admin_page,
};

impl Ui {
    pub(super) fn draw_admin_page(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        m: Metrics,
        top: f32,
        pt: f32,
    ) {
        self.draw_group_page(r, scene, f, m, top, pt, ADMIN_PAGE, "pause.page.admin.head", "pause.page.admin.note");
    }
}