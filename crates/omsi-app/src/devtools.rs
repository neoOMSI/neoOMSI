use imgui::{
    BackendFlags, Condition, DrawCmd, DrawCmdParams, Key, MouseButton, TextureId,
};
use omsi_sim::collision::Obb;
use omsi_render::{Renderer, devtools as rdev};
use winit::event::{ElementState, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

const SHADER: &str = r#"
struct U { a: vec4<f32>, b: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var t: texture_2d<f32>;
@group(0) @binding(2) var s: sampler;
struct VO {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) col: vec4<f32>,
};
@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) col: vec4<f32>) -> VO {
    var o: VO;
    o.pos = vec4<f32>(pos * u.a.xy + u.a.zw, 0.0, 1.0);
    o.uv = uv;
    var c = col;
    if (u.b.x > 0.5) {
        c = vec4<f32>(pow(c.rgb, vec3<f32>(2.2)), c.a);
    }
    o.col = c;
    return o;
}
@fragment
fn fs_main(i: VO) -> @location(0) vec4<f32> {
    return i.col * textureSample(t, s, i.uv);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vert {
    pos: [f32; 2],
    uv: [f32; 2],
    col: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    a: [f32; 4],
    b: [f32; 4],
}

struct Gpu {
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    vbuf: wgpu::Buffer,
    vcap: u64,
    ibuf: wgpu::Buffer,
    icap: u64,
}

pub(crate) struct Snapshot {
    pub adapter: String,
    pub format: wgpu::TextureFormat,
    pub surface: (u32, u32),
    pub dt_ms: f32,
    pub fps: f32,
    pub msaa: u32,
    pub anisotropy: u16,
    pub shadow_size: u32,
    pub ssao: bool,
    pub fxaa: bool,
    pub reflections: bool,
    pub render_scale: f32,
    pub meshes: usize,
    pub textures: usize,
    pub materials: usize,
    pub instances: usize,
    pub lights: usize,
    pub interior_lights: usize,
    pub coronas: usize,
}

pub(crate) struct FootInfo {
    pub pos: [f64; 3],
    pub heading: f64,
    pub vel: [f64; 2],
    pub vz: f64,
    pub on_lane: bool,
    pub attached: bool,
    pub seated: bool,
    pub inside: bool,
}

pub(crate) struct DoorDbg {
    pub outside: [f64; 3],
    pub inside: [f64; 3],
    pub open: bool,
    pub along: f64,
    pub len: f64,
    pub lateral: f64,
    pub in_lane: bool,
}

pub(crate) struct LanInfo {
    pub host: bool,
    pub connected: bool,
    pub code: Option<String>,
    pub target: Option<String>,
    pub local: Option<String>,
    pub peers: usize,
    pub name: String,
    pub session: u64,
    pub rejected: Option<String>,
    pub sent: u64,
    pub map: String,
}

pub(crate) struct TourRow {
    pub line: String,
    pub number: String,
    pub start: f64,
    pub trips: usize,
    pub available: bool,
}

pub(crate) struct Extra {
    pub map: String,
    pub clock: f64,
    pub paused: bool,
    pub cam: Option<omsi_render::Camera>,
    pub foot: Option<FootInfo>,
    pub boxes: Vec<Obb>,
    pub doors: Vec<DoorDbg>,
    pub blockers: Vec<Obb>,
    pub lan: Option<LanInfo>,
    pub tours: Vec<TourRow>,
    pub quicksave: bool,
}

pub(crate) enum Action {
    QuickSave,
    LoadQuickSave,
    CopyCode,
    OpenLan(u16),
    Connect(String),
}

struct Show {
    graphics: bool,
    lights: bool,
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

    pub(crate) fn event(&mut self, event: &WindowEvent) -> bool {
        if let WindowEvent::KeyboardInput { event: k, .. } = event {
            if k.physical_key == PhysicalKey::Code(KeyCode::Backquote) {
                if k.state == ElementState::Pressed && !k.repeat {
                    self.visible = !self.visible;
                }
                return true;
            }
        }
        if !self.visible {
            return false;
        }
        let io = self.ctx.io_mut();
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                io.add_mouse_pos_event([position.x as f32, position.y as f32]);
                false
            }
            WindowEvent::CursorLeft { .. } => {
                io.add_mouse_pos_event([-f32::MAX, -f32::MAX]);
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => return false,
                };
                let was_over = io.want_capture_mouse;
                io.add_mouse_button_event(b, *state == ElementState::Pressed);
                was_over && *state == ElementState::Pressed
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (*x, *y),
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        (p.x as f32 / 40.0, p.y as f32 / 40.0)
                    }
                };
                io.add_mouse_wheel_event([x, y]);
                io.want_capture_mouse
            }
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                io.add_key_event(Key::ModCtrl, s.control_key());
                io.add_key_event(Key::ModShift, s.shift_key());
                io.add_key_event(Key::ModAlt, s.alt_key());
                io.add_key_event(Key::ModSuper, s.super_key());
                false
            }
            WindowEvent::KeyboardInput { event: k, .. } => {
                let down = k.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = k.physical_key {
                    if let Some(key) = map_key(code) {
                        io.add_key_event(key, down);
                    }
                }
                if down {
                    if let Some(text) = k.text.as_deref() {
                        for c in text.chars().filter(|c| !c.is_control()) {
                            io.add_input_character(c);
                        }
                    }
                }
                io.want_capture_keyboard
            }
            _ => false,
        }
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
            let box_radius = &mut self.box_radius;
            let ui = self.ctx.new_frame();

            if *show_boxes {
                if let Some(cam) = extra.cam.as_ref() {
                    draw_boxes(ui, cam, (w, h), &extra.boxes, None);
                    draw_boxes(ui, cam, (w, h), &extra.blockers, Some([1.0, 0.1, 0.1, 1.0]));
                    draw_doors(ui, cam, (w, h), &extra.doors);
                }
            }

            if let Some(_bar) = ui.begin_main_menu_bar() {
                if let Some(_m) = ui.begin_menu("Game") {
                    if ui.menu_item_config("Map Info").selected(show.map).build() {
                        show.map = !show.map;
                    }
                    if ui
                        .menu_item_config("Tour Table")
                        .selected(show.tours)
                        .build()
                    {
                        show.tours = !show.tours;
                    }
                    ui.separator();
                    if ui.menu_item_config("Quick Save").shortcut("Ctrl+S").build() {
                        actions.push(Action::QuickSave);
                    }
                    if ui
                        .menu_item_config("Load Quick Save")
                        .enabled(extra.quicksave)
                        .build()
                    {
                        actions.push(Action::LoadQuickSave);
                    }
                }
                if let Some(_m) = ui.begin_menu("Net") {
                    if ui
                        .menu_item_config("Connect to Server...")
                        .enabled(extra.lan.is_none())
                        .build()
                    {
                        show.connect = true;
                    }
                    if ui
                        .menu_item_config("Open LAN...")
                        .enabled(extra.lan.is_none())
                        .build()
                    {
                        show.lan = true;
                    }
                    ui.separator();
                    if ui
                        .menu_item_config("Server Information")
                        .selected(show.server)
                        .enabled(extra.lan.is_some())
                        .build()
                    {
                        show.server = !show.server;
                    }
                    if ui
                        .menu_item_config("Copy Server Code")
                        .enabled(extra.lan.is_some())
                        .build()
                    {
                        actions.push(Action::CopyCode);
                    }
                }
                if let Some(_m) = ui.begin_menu("Graphics") {
                    if let Some(_r) = ui.begin_menu("Render Mode") {
                        for (i, n) in names.iter().enumerate() {
                            if ui.menu_item_config(n).selected(mode == i).build() {
                                mode = i;
                            }
                        }
                    }
                    if ui
                        .menu_item_config("Wireframe")
                        .selected(mode == 1)
                        .build()
                    {
                        mode = if mode == 1 { 0 } else { 1 };
                    }
                    if ui.menu_item("Reset Render Mode") {
                        mode = 0;
                    }
                    ui.separator();
                    if ui
                        .menu_item_config("Graphics Window")
                        .selected(show.graphics)
                        .build()
                    {
                        show.graphics = !show.graphics;
                    }
                }
                if let Some(_m) = ui.begin_menu("Lights") {
                    if ui
                        .menu_item_config("Light Settings")
                        .selected(show.lights)
                        .build()
                    {
                        show.lights = !show.lights;
                    }
                    if ui.menu_item("Reset Light Settings") {
                        crate::lights::set_settings(crate::lights::LightSettings::DEFAULT);
                    }
                }
                if let Some(_m) = ui.begin_menu("Walk") {
                    if ui
                        .menu_item_config("Walk Details")
                        .selected(show.walk)
                        .build()
                    {
                        show.walk = !show.walk;
                    }
                    if ui
                        .menu_item_config("Show Hitboxes")
                        .selected(*show_boxes)
                        .build()
                    {
                        *show_boxes = !*show_boxes;
                    }
                    if ui
                        .menu_item_config("Show Mesh (Wireframe)")
                        .selected(mode == 1)
                        .build()
                    {
                        mode = if mode == 1 { 0 } else { 1 };
                    }
                }
            }

            if show.lights {
                let mut s = crate::lights::settings();
                ui.window("Light Settings")
                    .opened(&mut show.lights)
                    .size([400.0, 380.0], Condition::FirstUseEver)
                    .position([12.0, 32.0], Condition::FirstUseEver)
                    .build(|| {
                        ui.checkbox("Force High Beam", &mut s.force_high_beam);
                        ui.separator();
                        ui.slider("Headlight", 0.0, 100.0, &mut s.headlight);
                        ui.slider("Vanilla Headlight", 0.0, 2.0, &mut s.vanilla);
                        ui.slider("Low Beam Gain", 1.0, 10.0, &mut s.low_beam_gain);
                        ui.separator();
                        ui.slider("High Beam Gain", 0.0, 3.0, &mut s.high_beam);
                        ui.slider("High Beam Range", 0.5, 4.0, &mut s.high_beam_range);
                        ui.slider("High Beam Spread", 0.25, 3.0, &mut s.high_beam_spread);
                        ui.separator();
                        ui.slider("Weather Boost", 0.0, 4.0, &mut s.weather_boost);
                        ui.slider("Weather Night", 0.0, 1.0, &mut s.weather_night);
                        ui.slider("Corona / Cone", 0.0, 4.0, &mut s.corona);
                        ui.separator();
                        if ui.button("Reset") {
                            s = crate::lights::LightSettings::DEFAULT;
                        }
                    });
                crate::lights::set_settings(s);
            }

            if show.graphics {
                ui.window("Graphics")
                    .opened(&mut show.graphics)
                    .size([400.0, 420.0], Condition::FirstUseEver)
                    .position([12.0, 32.0], Condition::FirstUseEver)
                    .build(|| {
                        ui.text(format!("GPU: {}", snap.adapter));
                        ui.text(format!("Format: {:?}", snap.format));
                        ui.text(format!("Window: {} x {}", snap.surface.0, snap.surface.1));
                        ui.text(format!("{:.0} FPS, {:.2} ms", snap.fps, snap.dt_ms));
                        let worst = history.iter().cloned().fold(0.0f32, f32::max);
                        let overlay = format!("max {worst:.1} ms");
                        ui.plot_lines("##ms", history)
                            .scale_min(0.0)
                            .scale_max(worst.max(1.0))
                            .graph_size([0.0, 56.0])
                            .overlay_text(&overlay)
                            .build();
                        ui.separator();
                        ui.text(format!("Render mode: {}", names[mode.min(names.len() - 1)]));
                        if mode == 1 && !rdev::wireframe_supported() {
                            ui.text_colored(
                                [1.0, 0.6, 0.2, 1.0],
                                "Wireframe: GPU lacks POLYGON_MODE_LINE",
                            );
                        }
                        if mode >= 2 {
                            ui.text_wrapped("Debug views need the enhanced graphics.");
                        }
                        ui.separator();
                        ui.text(format!(
                            "MSAA {}x, AF {}x, Shadows {}, Render scale {}",
                            snap.msaa,
                            snap.anisotropy,
                            snap.shadow_size,
                            if snap.render_scale <= 0.0 {
                                "auto".to_string()
                            } else {
                                format!("{:.2}", snap.render_scale)
                            }
                        ));
                        ui.text(format!(
                            "SSAO {}, FXAA {}, Reflections {}",
                            on_off(snap.ssao),
                            on_off(snap.fxaa),
                            on_off(snap.reflections)
                        ));
                        ui.separator();
                        ui.text(format!(
                            "Meshes {}, Textures {}, Materials {}",
                            snap.meshes, snap.textures, snap.materials
                        ));
                        ui.text(format!(
                            "Instances {}, Lights {} (interior {}), Coronas {}",
                            snap.instances, snap.lights, snap.interior_lights, snap.coronas
                        ));
                    });
            }
            if show.map {
                ui.window("Map Info")
                    .opened(&mut show.map)
                    .size([380.0, 260.0], Condition::FirstUseEver)
                    .build(|| {
                        ui.text(format!("Map: {}", extra.map));
                        ui.text(format!("Time: {}", hms(extra.clock)));
                        ui.text(format!("Paused: {}", extra.paused));
                        if let Some(c) = extra.cam.as_ref() {
                            ui.text(format!(
                                "Camera: {:.1} {:.1} {:.1}  yaw {:.0} pitch {:.0}",
                                c.position.x, c.position.y, c.position.z, c.yaw, c.pitch
                            ));
                        }
                        ui.separator();
                        ui.text(format!(
                            "Meshes {}, Textures {}, Materials {}",
                            snap.meshes, snap.textures, snap.materials
                        ));
                        ui.text(format!(
                            "Instances {}, Lights {} (interior {}), Coronas {}",
                            snap.instances, snap.lights, snap.interior_lights, snap.coronas
                        ));
                    });
            }
            if show.tours {
                ui.window("Tour Table")
                    .opened(&mut show.tours)
                    .size([520.0, 360.0], Condition::FirstUseEver)
                    .build(|| {
                        if extra.tours.is_empty() {
                            ui.text("No timetable loaded.");
                            return;
                        }
                        ui.text(format!("Now: {}", hms(extra.clock)));
                        ui.columns(5, "##tours", true);
                        for h in ["Line", "Tour", "Start", "Trips", "Today"] {
                            ui.text(h);
                            ui.next_column();
                        }
                        ui.separator();
                        for t in &extra.tours {
                            ui.text(&t.line);
                            ui.next_column();
                            ui.text(&t.number);
                            ui.next_column();
                            ui.text(hms(t.start));
                            ui.next_column();
                            ui.text(t.trips.to_string());
                            ui.next_column();
                            ui.text(if t.available { "yes" } else { "no" });
                            ui.next_column();
                        }
                        ui.columns(1, "##toursend", false);
                    });
            }
            if show.server {
                ui.window("Server Information")
                    .opened(&mut show.server)
                    .size([400.0, 260.0], Condition::FirstUseEver)
                    .build(|| {
                        let Some(l) = extra.lan.as_ref() else {
                            ui.text("Not in a LAN session or on a server.");
                            return;
                        };
                        ui.text(format!("Role: {}", if l.host { "Host" } else { "Client" }));
                        ui.text(format!("Connected: {}", l.connected));
                        ui.text(format!("Name: {}", l.name));
                        ui.text(format!("Session: {:016x}", l.session));
                        ui.text(format!("Map: {}", l.map));
                        ui.text(format!("Other players: {}", l.peers));
                        ui.text(format!("Sent: {} KiB", l.sent / 1024));
                        if let Some(a) = l.local.as_ref() {
                            ui.text(format!("Local address: {a}"));
                        }
                        if let Some(a) = l.target.as_ref() {
                            ui.text(format!("Target: {a}"));
                        }
                        if let Some(c) = l.code.as_ref() {
                            ui.text_wrapped(format!("Code: {c}"));
                            if ui.button("Copy code") {
                                actions.push(Action::CopyCode);
                            }
                        }
                        if let Some(r) = l.rejected.as_ref() {
                            ui.text_colored([1.0, 0.4, 0.3, 1.0], format!("Rejected: {r}"));
                        }
                    });
            }
            if show.connect {
                ui.window("Connect to Server")
                    .opened(&mut show.connect)
                    .size([380.0, 110.0], Condition::FirstUseEver)
                    .build(|| {
                        ui.text("Address, code or https:// name:");
                        ui.set_next_item_width(-1.0);
                        ui.input_text("##addr", &mut *connect_addr).build();
                        let go = !connect_addr.trim().is_empty();
                        if ui.button("Connect") && go {
                            actions.push(Action::Connect(connect_addr.trim().to_string()));
                        }
                        ui.same_line();
                        ui.text_disabled("(restarts the game and joins)");
                    });
            }
            if show.lan {
                ui.window("Open LAN")
                    .opened(&mut show.lan)
                    .size([320.0, 110.0], Condition::FirstUseEver)
                    .build(|| {
                        ui.input_int("UDP port (0 = default)", &mut *lan_port).build();
                        *lan_port = (*lan_port).clamp(0, 65535);
                        if ui.button("Open") {
                            actions.push(Action::OpenLan(*lan_port as u16));
                        }
                    });
            }
            if show.walk {
                ui.window("Walk Details")
                    .opened(&mut show.walk)
                    .size([380.0, 280.0], Condition::FirstUseEver)
                    .build(|| {
                        match extra.foot.as_ref() {
                            Some(f) => {
                                ui.text(format!(
                                    "Pos: {:.2} {:.2} {:.2}",
                                    f.pos[0], f.pos[1], f.pos[2]
                                ));
                                ui.text(format!("Heading: {:.1}", f.heading));
                                ui.text(format!(
                                    "Velocity: {:.2} {:.2}, vertical {:.2}",
                                    f.vel[0], f.vel[1], f.vz
                                ));
                                ui.text(format!(
                                    "On lane {}, attached {}, seated {}, in bus {}",
                                    f.on_lane, f.attached, f.seated, f.inside
                                ));
                            }
                            None => ui.text("Not walking."),
                        }
                        ui.separator();
                        ui.checkbox("Show hitboxes", &mut *show_boxes);
                        ui.slider("Radius (m)", 5.0, 80.0, &mut *box_radius);
                        if mode == 1 {
                            ui.text("Mesh: wireframe on");
                        } else if ui.button("Show mesh (wireframe)") {
                            mode = 1;
                        }
                        ui.text(format!("Boxes shown: {}", extra.boxes.len()));
                        ui.separator();
                        ui.text(format!("Blocking boxes: {} (red)", extra.blockers.len()));
                        for o in extra.blockers.iter().take(6) {
                            ui.text(format!(
                                "id {} z {:.2}..{:.2} half {:.2} {:.2} {}",
                                o.id,
                                o.z0,
                                o.z1,
                                o.half.x,
                                o.half.y,
                                if o.mass > 0.0 { "vehicle" } else { "scenery" }
                            ));
                        }
                        ui.separator();
                        ui.text(format!("Bus doors near: {}", extra.doors.len()));
                        for (i, d) in extra.doors.iter().enumerate() {
                            ui.text_colored(
                                if d.in_lane {
                                    [0.3, 1.0, 0.4, 1.0]
                                } else if d.open {
                                    [1.0, 0.8, 0.3, 1.0]
                                } else {
                                    [1.0, 0.4, 0.3, 1.0]
                                },
                                format!(
                                    "#{i} open {} along {:.1}/{:.1} lateral {:.1} lane {}",
                                    d.open, d.along, d.len, d.lateral, d.in_lane
                                ),
                            );
                        }
                        ui.text_disabled("door line: green open, red closed; lane needs along -1.2..len+1, lateral <= 1.0");
                        ui.text_disabled("green scenery, orange vehicles, yellow poles");
                    });
            }
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
        let draw_data = self.ctx.render();
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let device = &r.device;
        let queue = &r.queue;
        let mut verts: Vec<Vert> = Vec::with_capacity(draw_data.total_vtx_count as usize);
        let mut idx: Vec<u16> = Vec::with_capacity(draw_data.total_idx_count as usize);
        let mut lists: Vec<(u32, u32)> = Vec::new();
        for list in draw_data.draw_lists() {
            lists.push((verts.len() as u32, idx.len() as u32));
            verts.extend(list.vtx_buffer().iter().map(|v| Vert {
                pos: v.pos,
                uv: v.uv,
                col: v.col,
            }));
            idx.extend_from_slice(list.idx_buffer());
        }
        if verts.is_empty() || idx.is_empty() {
            return;
        }
        if idx.len() % 2 == 1 {
            idx.push(0);
        }
        let vbytes = (verts.len() * size_of::<Vert>()) as u64;
        let ibytes = (idx.len() * 2) as u64;
        if vbytes > gpu.vcap {
            gpu.vcap = (vbytes * 2).next_multiple_of(4);
            gpu.vbuf = buffer(device, wgpu::BufferUsages::VERTEX, gpu.vcap);
        }
        if ibytes > gpu.icap {
            gpu.icap = (ibytes * 2).next_multiple_of(4);
            gpu.ibuf = buffer(device, wgpu::BufferUsages::INDEX, gpu.icap);
        }
        queue.write_buffer(&gpu.vbuf, 0, bytemuck::cast_slice(&verts));
        queue.write_buffer(&gpu.ibuf, 0, bytemuck::cast_slice(&idx));
        let u = Uniform {
            a: [2.0 / w as f32, -2.0 / h as f32, -1.0, 1.0],
            b: [if gpu.format.is_srgb() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&gpu.uniform, 0, bytemuck::bytes_of(&u));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("devtools"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("devtools"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, &gpu.group, &[]);
            pass.set_vertex_buffer(0, gpu.vbuf.slice(..));
            pass.set_index_buffer(gpu.ibuf.slice(..), wgpu::IndexFormat::Uint16);
            for (list, &(vbase, ibase)) in draw_data.draw_lists().zip(&lists) {
                for cmd in list.commands() {
                    if let DrawCmd::Elements {
                        count,
                        cmd_params:
                        DrawCmdParams {
                            clip_rect,
                            vtx_offset,
                            idx_offset,
                            ..
                        },
                    } = cmd
                    {
                        let x0 = clip_rect[0].clamp(0.0, w as f32) as u32;
                        let y0 = clip_rect[1].clamp(0.0, h as f32) as u32;
                        let x1 = clip_rect[2].clamp(0.0, w as f32) as u32;
                        let y1 = clip_rect[3].clamp(0.0, h as f32) as u32;
                        if x1 <= x0 || y1 <= y0 {
                            continue;
                        }
                        pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
                        let first = ibase + idx_offset as u32;
                        pass.draw_indexed(
                            first..first + count as u32,
                            (vbase + vtx_offset as u32) as i32,
                            0..1,
                        );
                    }
                }
            }
        }
        queue.submit(Some(encoder.finish()));
    }

    fn ensure_gpu(&mut self, r: &Renderer) {
        let format = r.format();
        if self.gpu.as_ref().is_some_and(|g| g.format == format) {
            return;
        }
        let device = &r.device;
        let queue = &r.queue;
        if self.font_texture.is_none() {
            let (fw, fh) = self.font_size;
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("devtools font"),
                size: wgpu::Extent3d {
                    width: fw,
                    height: fh,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &self.font_rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(fw * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: fw,
                    height: fh,
                    depth_or_array_layers: 1,
                },
            );
            self.font_texture = Some(tex.create_view(&Default::default()));
        }
        let font_view = self.font_texture.as_ref().unwrap();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("devtools"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("devtools"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<Uniform>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("devtools"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4];
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("devtools"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vert>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                })],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = buffer(
            device,
            wgpu::BufferUsages::UNIFORM,
            size_of::<Uniform>() as u64,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("devtools"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("devtools"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(font_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        self.gpu = Some(Gpu {
            format,
            pipeline,
            uniform,
            group,
            vbuf: buffer(device, wgpu::BufferUsages::VERTEX, 65536),
            vcap: 65536,
            ibuf: buffer(device, wgpu::BufferUsages::INDEX, 65536),
            icap: 65536,
        });
    }
}

fn buffer(device: &wgpu::Device, usage: wgpu::BufferUsages, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("devtools"),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn map_key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::ArrowLeft => Key::LeftArrow,
        KeyCode::ArrowRight => Key::RightArrow,
        KeyCode::ArrowUp => Key::UpArrow,
        KeyCode::ArrowDown => Key::DownArrow,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyV => Key::V,
        KeyCode::KeyX => Key::X,
        _ => return None,
    })
}

fn hms(t: f64) -> String {
    let t = t.max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", (t / 3600) % 24, (t / 60) % 60, t % 60)
}

fn project(vp: &glam::Mat4, cam: glam::DVec3, p: glam::DVec3, size: (u32, u32)) -> Option<[f32; 2]> {
    let c = vp.mul_vec4((p - cam).as_vec3().extend(1.0));
    if c.w <= 0.01 {
        return None;
    }
    let (x, y) = (c.x / c.w, c.y / c.w);
    Some([
        (x * 0.5 + 0.5) * size.0 as f32,
        (0.5 - y * 0.5) * size.1 as f32,
    ])
}

fn draw_boxes(
    ui: &imgui::Ui,
    cam: &omsi_render::Camera,
    size: (u32, u32),
    boxes: &[Obb],
    over: Option<[f32; 4]>,
) {
    let vp = cam.view_proj(size.0 as f32 / size.1.max(1) as f32, cam.position);
    let list = ui.get_background_draw_list();
    for o in boxes {
        let col = if let Some(c) = over {
            c
        } else if o.pole.is_some() {
            [1.0, 0.95, 0.2, 0.9]
        } else if o.mass > 0.0 || o.id < -1 {
            [1.0, 0.55, 0.1, 0.9]
        } else {
            [0.2, 1.0, 0.3, 0.8]
        };
        let (sh, ch) = o.heading.sin_cos();
        let corner = |sx: f64, sy: f64, z: f64| {
            let (lx, ly) = (sx * o.half.x, sy * o.half.y);
            glam::DVec3::new(
                o.center.x + lx * ch + ly * sh,
                o.center.y - lx * sh + ly * ch,
                z,
            )
        };
        let signs = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        let mut lo = [None; 4];
        let mut hi = [None; 4];
        for (i, (sx, sy)) in signs.iter().enumerate() {
            lo[i] = project(&vp, cam.position, corner(*sx, *sy, o.z0), size);
            hi[i] = project(&vp, cam.position, corner(*sx, *sy, o.z1), size);
        }
        for i in 0..4 {
            let j = (i + 1) % 4;
            for (a, b) in [(lo[i], lo[j]), (hi[i], hi[j]), (lo[i], hi[i])] {
                if let (Some(a), Some(b)) = (a, b) {
                    list.add_line(a, b, col)
                        .thickness(if over.is_some() { 3.0 } else { 1.0 })
                        .build();
                }
            }
        }
    }
}

#[cfg(all(feature = "devtools", debug_assertions))]
impl crate::App {
    pub(crate) fn dev_gather(&self) -> Extra {
        let (boxes_on, radius, tours_on) = self
            .devtools
            .as_ref()
            .map_or((false, 25.0, false), |d| {
                (d.wants_boxes(), d.box_radius(), d.wants_tours())
            });
        let foot = self.on_foot.as_ref().map(|f| FootInfo {
            pos: [f.pos.x, f.pos.y, f.pos.z],
            heading: f.heading,
            vel: [f.vel.x, f.vel.y],
            vz: f.vz,
            on_lane: f.on_lane,
            attached: f.attached,
            seated: f.seat.is_some(),
            inside: f.inside.is_some(),
        });
        let doors = match (self.on_foot.as_ref(), self.humans.as_ref()) {
            (Some(f), Some(hm)) if boxes_on || self.devtools.is_some() => {
                let mut v = Vec::new();
                for bus in hm.bus_ids_near(f.pos, 25.0) {
                    for (inside, outside, _, open) in hm.cabin_doors(bus) {
                        let Some((wi, _)) = hm.cabin_world(bus, inside) else {
                            continue;
                        };
                        let (a, b) = (outside.truncate(), wi.truncate());
                        let ab = b - a;
                        let len = ab.length();
                        if len < 1e-3 {
                            continue;
                        }
                        let dir = ab / len;
                        let rel = f.pos.truncate() - a;
                        let (along, lateral) = (rel.dot(dir), rel.perp_dot(dir).abs());
                        v.push(DoorDbg {
                            outside: [outside.x, outside.y, outside.z],
                            inside: [wi.x, wi.y, wi.z],
                            open,
                            along,
                            len,
                            lateral,
                            in_lane: open && along >= -1.2 && along <= len + 1.0 && lateral <= 1.0,
                        });
                    }
                }
                v
            }
            _ => Vec::new(),
        };
        let boxes = if boxes_on {
            let at = self
                .on_foot
                .as_ref()
                .map(|f| f.pos)
                .or_else(|| self.camera.as_ref().map(|c| c.position));
            at.map(|at| self.dev_hitboxes(at, radius)).unwrap_or_default()
        } else {
            Vec::new()
        };
        let blockers = if boxes_on {
            let exempt = doors
                .iter()
                .filter(|d| d.in_lane)
                .min_by(|a, b| {
                    a.lateral
                        .partial_cmp(&b.lateral)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|d| glam::DVec2::new(d.inside[0], d.inside[1]));
            self.dev_blockers(exempt)
        } else {
            Vec::new()
        };
        let lan = self.lan.as_ref().map(|l| LanInfo {
            host: l.role == omsi_net::Role::Host,
            connected: l.connected,
            code: l.code().map(|c| c.encode()),
            target: self.args.lan_join.clone().or(l.host.map(|a| a.to_string())),
            local: l.local_addr().map(|a| a.to_string()),
            peers: l.peer_count(),
            name: l.my_name.clone(),
            session: l.session,
            rejected: l.rejected.clone(),
            sent: l.sent(),
            map: l.world.map.clone(),
        });
        let mut tours = Vec::new();
        if tours_on {
            if let Some(sch) = self.schedule.as_ref() {
                for line in &sch.data.lines {
                    for t in &line.tours {
                        tours.push(TourRow {
                            line: line.name.clone(),
                            number: t.number.clone(),
                            start: crate::game_lists::tour_start(t).unwrap_or(0.0),
                            trips: t.trips.len(),
                            available: sch.tour_available(t),
                        });
                    }
                }
                tours.sort_by(|a, b| {
                    a.line.cmp(&b.line).then(
                        a.start
                            .partial_cmp(&b.start)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
                });
                tours.truncate(3000);
            }
        }
        let quicksave = crate::startup::content_dir()
            .unwrap_or_else(|| self.args.root.clone())
            .join("Situations")
            .join("quicksave.osn")
            .exists();
        Extra {
            map: self.args.map.clone(),
            clock: self.clock.time,
            paused: self.paused,
            cam: self.camera,
            foot,
            boxes,
            doors,
            blockers,
            lan,
            tours,
            quicksave,
        }
    }

    pub(crate) fn dev_actions(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let actions = match self.devtools.as_mut() {
            Some(d) => d.take_actions(),
            None => return,
        };
        for a in actions {
            match a {
                Action::QuickSave => self.quick_save(),
                Action::LoadQuickSave => {
                    if self.load_quicksave() {
                        self.finish_session();
                        crate::platform::exit(event_loop);
                    }
                }
                Action::CopyCode => self.copy_server_code(),
                Action::OpenLan(port) => {
                    if self.lan.is_some() {
                        self.service_msg = Some(("Already in a LAN session".into(), 3.0));
                        continue;
                    }
                    let (p, try_next) = if port == 0 {
                        (omsi_net::DEFAULT_PORT, true)
                    } else {
                        (port, false)
                    };
                    match omsi_net::LanSession::host(
                        p,
                        &crate::lan::player_name(&self.args),
                        crate::lan::world_info(&self.args),
                        try_next,
                    ) {
                        Ok(s) => {
                            self.lan = Some(s);
                            self.copy_server_code();
                        }
                        Err(e) => {
                            self.service_msg =
                                Some((format!("Cannot open LAN on port {p}: {e}"), 5.0));
                        }
                    }
                }
                Action::Connect(addr) => {
                    let Ok(exe) = std::env::current_exe() else {
                        continue;
                    };
                    let mut cmd = std::process::Command::new(exe);
                    cmd.arg("--root")
                        .arg(&self.args.root)
                        .arg("--no-menu")
                        .arg("--lan-join")
                        .arg(&addr);
                    match cmd.spawn() {
                        Ok(_) => {
                            self.finish_session();
                            crate::platform::exit(event_loop);
                        }
                        Err(e) => {
                            self.service_msg =
                                Some((format!("Could not start the game: {e}"), 5.0));
                        }
                    }
                }
            }
        }
    }
}

fn draw_doors(ui: &imgui::Ui, cam: &omsi_render::Camera, size: (u32, u32), doors: &[DoorDbg]) {
    let vp = cam.view_proj(size.0 as f32 / size.1.max(1) as f32, cam.position);
    let list = ui.get_background_draw_list();
    for d in doors {
        let col = if d.open {
            [0.2, 1.0, 0.3, 1.0]
        } else {
            [1.0, 0.2, 0.2, 1.0]
        };
        let a = glam::DVec3::new(d.outside[0], d.outside[1], d.outside[2] + 0.1);
        let b = glam::DVec3::new(d.inside[0], d.inside[1], d.inside[2] + 0.1);
        if let (Some(pa), Some(pb)) = (
            project(&vp, cam.position, a, size),
            project(&vp, cam.position, b, size),
        ) {
            list.add_line(pa, pb, col).thickness(3.0).build();
            list.add_circle(pa, 5.0, col).filled(true).build();
        }
    }
}