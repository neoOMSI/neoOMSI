//! Developer tools (ImGui): the overlay windows, menu and debug drawing.
#![allow(unused_imports)]

mod gather;
mod gpu;
mod info;
mod input;
mod lights_ui;
mod menu;
mod net;
mod overlays;
mod types;
mod util;
mod vehicle_editor;
mod vehicle_ui;
mod walk;

use imgui::{BackendFlags, Condition, TextureId};
use omsi_render::{Renderer, devtools as rdev};

use gpu::Gpu;
use overlays::{draw_beams, draw_boxes, draw_doors};

pub(crate) use types::*;
pub(crate) use util::project;

struct Show {
    graphics: bool,
    lights: bool,
    vehicle: bool,
    cockpit: bool,
    map: bool,
    tours: bool,
    server: bool,
    connect: bool,
    lan: bool,
    walk: bool,
}

pub(crate) struct DevTools {
    ctx: imgui::Context,
    gpu: Option<Gpu>,
    font_texture: Option<wgpu::TextureView>,
    font_size: (u32, u32),
    font_rgba: Vec<u8>,
    visible: bool,
    mode: usize,
    mode_names: Vec<String>,
    history: Vec<f32>,
    show: Show,
    actions: Vec<Action>,
    connect_addr: String,
    lan_port: i32,
    show_boxes: bool,
    box_radius: f32,
    vehicle_filter: String,
    cockpit_filter: String,
    release: Vec<String>,
    editor: vehicle_editor::VehicleEditor,
}

impl DevTools {
    pub(crate) fn new() -> DevTools {
        let mut ctx = imgui::Context::create();
        ctx.set_ini_filename(None);
        ctx.set_log_filename(None);
        ctx.io_mut().backend_flags |= BackendFlags::RENDERER_HAS_VTX_OFFSET;
        ctx.style_mut().use_dark_colors();
        let (font_size, font_rgba) = {
            let fonts = ctx.fonts();
            fonts.add_font(&[imgui::FontSource::DefaultFontData { config: None }]);
            let tex = fonts.build_rgba32_texture();
            ((tex.width, tex.height), tex.data.to_vec())
        };
        ctx.fonts().tex_id = TextureId::new(0);
        let mut mode_names = vec!["Normal".to_string(), "Wireframe".to_string()];
        for (_, n) in rdev::DEBUG_VIEWS.iter().skip(1) {
            mode_names.push(format!("Debug: {n}"));
        }
        DevTools {
            ctx,
            gpu: None,
            font_texture: None,
            font_size,
            font_rgba,
            visible: true,
            mode: 0,
            mode_names,
            history: Vec::new(),
            show: Show {
                graphics: false,
                lights: false,
                vehicle: false,
                cockpit: false,
                map: false,
                tours: false,
                server: false,
                connect: false,
                lan: false,
                walk: false,
            },
            actions: Vec::new(),
            connect_addr: String::new(),
            lan_port: 0,
            show_boxes: false,
            vehicle_filter: String::new(),
            cockpit_filter: String::new(),
            release: Vec::new(),
            editor: vehicle_editor::VehicleEditor::new(),
            box_radius: 25.0,
        }
    }

    pub(crate) fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }

    pub(crate) fn wants_boxes(&self) -> bool {
        self.visible && self.show_boxes
    }

    pub(crate) fn box_radius(&self) -> f64 {
        self.box_radius as f64
    }

    pub(crate) fn wants_tours(&self) -> bool {
        self.visible && self.show.tours
    }

    pub(crate) fn render(
        &mut self,
        r: &Renderer,
        view: &wgpu::TextureView,
        scale: f32,
        snap: &Snapshot,
        extra: &Extra,
    ) {
        if !self.visible {
            return;
        }
        let (w, h) = snap.surface;
        if w == 0 || h == 0 {
            return;
        }
        self.history.push(snap.dt_ms);
        if self.history.len() > 240 {
            self.history.remove(0);
        }
        {
            let io = self.ctx.io_mut();
            io.display_size = [w as f32, h as f32];
            io.font_global_scale = scale.max(1.0);
            io.delta_time = (snap.dt_ms / 1000.0).clamp(0.0001, 0.25);
        }
        self.ensure_gpu(r);
        let mut mode = self.mode;
        {
            let names = &self.mode_names;
            let history = &self.history;
            let show = &mut self.show;
            let actions = &mut self.actions;
            let connect_addr = &mut self.connect_addr;
            let lan_port = &mut self.lan_port;
            let show_boxes = &mut self.show_boxes;
            let vehicle_filter = &mut self.vehicle_filter;
            let cockpit_filter = &mut self.cockpit_filter;
            let editor = &mut self.editor;
            let ui = self.ctx.new_frame();

            if *show_boxes {
                if let Some(cam) = extra.cam.as_ref() {
                    draw_boxes(ui, cam, (w, h), &extra.boxes, None);
                    draw_boxes(ui, cam, (w, h), &extra.blockers, Some([1.0, 0.1, 0.1, 1.0]));
                    draw_doors(ui, cam, (w, h), &extra.doors);
                }
            }
            vehicle_editor::draw_world(ui, editor, extra, (w, h));
            if !extra.beams.is_empty() {
                if let Some(cam) = extra.cam.as_ref() {
                    draw_beams(ui, cam, (w, h), &extra.beams);
                }
            }

            menu::draw(ui, show, actions, extra, editor, names, &mut mode);

            lights_ui::lights_window(ui, &mut show.lights);
            if editor.open {
                let s = std::cell::RefCell::new(crate::lights::settings());
                vehicle_editor::window(
                    ui,
                    editor,
                    extra,
                    |ui| lights_ui::vehicle_panel(ui, &mut s.borrow_mut()),
                    |ui| lights_ui::spots_panel(ui, &mut s.borrow_mut(), extra),
                    |ui| lights_ui::interior_panel(ui, extra, actions),
                );
                crate::lights::set_settings(s.into_inner());
            }
            vehicle_ui::cockpit(ui, &mut show.cockpit, extra, cockpit_filter, actions);
            vehicle_ui::actions(ui, &mut show.vehicle, extra, vehicle_filter, actions);
            info::graphics(ui, &mut show.graphics, snap, history, names, mode);
            info::map(ui, &mut show.map, extra, snap);
            info::tours(ui, &mut show.tours, extra);
            net::server(ui, &mut show.server, extra, actions);
            net::connect(ui, &mut show.connect, connect_addr, actions);
            net::lan(ui, &mut show.lan, lan_port, actions);
            walk::window(ui, &mut show.walk, extra);
        }
        if mode != self.mode {
            self.mode = mode;
            rdev::set_wireframe(mode == 1);
            rdev::set_debug_view(if mode >= 2 {
                rdev::DEBUG_VIEWS.get(mode - 1).map(|v| v.0)
            } else {
                None
            });
        }
        self.submit(r, view, w, h);
    }
}
