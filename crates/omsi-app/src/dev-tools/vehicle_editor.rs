use crate::devtools::Extra;
use glam::{DVec3, Mat4, Vec3};

pub(crate) struct EditPath {
    pub name: String,
    pub closed: bool,
    pub points: Vec<[f32; 3]>,
}

pub(crate) struct VehicleEditor {
    pub open: bool,
    pub paths: Vec<EditPath>,
    pub sel_path: usize,
    pub sel_point: usize,
    pub draw: bool,
    pub show_walk: bool,
    pub show_interior: bool,
    pub status: String,
}

impl VehicleEditor {
    pub(crate) fn new() -> Self {
        VehicleEditor {
            open: false,
            paths: vec![EditPath {
                name: "path1".into(),
                closed: false,
                points: Vec::new(),
            }],
            sel_path: 0,
            sel_point: 0,
            draw: true,
            show_walk: true,
            show_interior: true,
            status: String::new(),
        }
    }

    fn export_text(&self) -> String {
        let mut s = String::new();
        for p in &self.paths {
            s.push_str(&format!(
                "[path]\n{}\n{}\n{}\n",
                p.name,
                p.closed as i32,
                p.points.len()
            ));
            for q in &p.points {
                s.push_str(&format!("{:.3}\n{:.3}\n{:.3}\n", q[0], q[1], q[2]));
            }
            s.push('\n');
        }
        s
    }

    fn export(&mut self) {
        let dir = crate::startup::content_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let file = dir.join("vehicle_paths.txt");
        self.status = match std::fs::write(&file, self.export_text()) {
            Ok(()) => format!("Saved {}", file.display()),
            Err(e) => format!("Save failed: {e}"),
        };
    }
}

pub(crate) fn window(
    ui: &imgui::Ui,
    ed: &mut VehicleEditor,
    extra: &Extra,
    lights: impl FnOnce(&imgui::Ui),
    interior: impl FnOnce(&imgui::Ui),
) {
    let mut open = ed.open;
    ui.window("Vehicle Editor")
        .opened(&mut open)
        .size([420.0, 560.0], imgui::Condition::FirstUseEver)
        .position([12.0, 32.0], imgui::Condition::FirstUseEver)
        .build(|| {
            if extra.pose.is_none() {
                ui.text_disabled("No vehicle driven");
            }
            if let Some(_bar) = ui.tab_bar("##vehicle_editor_tabs") {
                if let Some(_t) = ui.tab_item("Lights") {
                    lights(ui);
                }
                if let Some(_t) = ui.tab_item("Interior Lights") {
                    ui.checkbox("Show sources in world", &mut ed.show_interior);
                    interior(ui);
                }
                if let Some(_t) = ui.tab_item("Paths") {
                    paths_tab(ui, ed, extra);
                }
            }
        });
    ed.open = open;
}

fn paths_tab(ui: &imgui::Ui, ed: &mut VehicleEditor, extra: &Extra) {
    ui.checkbox("Draw my paths in world", &mut ed.draw);
    match extra.vehicle.as_ref() {
        Some(v) => {
            ui.checkbox("Draw vehicle passenger paths (green)", &mut ed.show_walk);
            ui.text_disabled(format!(
                "Vehicle paths: {} points, {} links",
                v.walk_points.len(),
                v.walk_links.len()
            ));
        }
        None => ui.text_disabled("Paths are drawn on a driven vehicle"),
    }
    if ui.button("New Path") {
        let n = ed.paths.len() + 1;
        ed.paths.push(EditPath {
            name: format!("path{n}"),
            closed: false,
            points: Vec::new(),
        });
        ed.sel_path = ed.paths.len() - 1;
        ed.sel_point = 0;
    }
    ui.same_line();
    if ui.button("Delete Path") && ed.sel_path < ed.paths.len() {
        ed.paths.remove(ed.sel_path);
        ed.sel_path = ed.sel_path.saturating_sub(1);
        ed.sel_point = 0;
    }
    ui.same_line();
    if ui.button("Export") {
        ed.export();
    }
    if !ed.status.is_empty() {
        ui.text_wrapped(&ed.status);
    }
    ui.separator();
    for i in 0..ed.paths.len() {
        let label = format!("{} ({})##p{i}", ed.paths[i].name, ed.paths[i].points.len());
        if ui
            .selectable_config(label)
            .selected(i == ed.sel_path)
            .build()
        {
            ed.sel_path = i;
            ed.sel_point = 0;
        }
    }
    let Some(path) = ed.paths.get_mut(ed.sel_path) else {
        ui.text_disabled("No path yet: press New Path");
        return;
    };
    ui.separator();
    ui.input_text("Name##path", &mut path.name).build();
    ui.checkbox("Closed", &mut path.closed);
    if ui.button("Add Point") {
        let next = match path.points.last() {
            Some(l) => [l[0], l[1] + 1.0, l[2]],
            None => [0.0, 0.0, 0.0],
        };
        path.points.push(next);
        ed.sel_point = path.points.len() - 1;
    }
    let sel = ed.sel_point.min(path.points.len().saturating_sub(1));
    ed.sel_point = sel;
    if !path.points.is_empty() {
        ui.same_line();
        if ui.button("Insert After") {
            let a = path.points[sel];
            let b = path
                .points
                .get(sel + 1)
                .copied()
                .unwrap_or([a[0], a[1] + 1.0, a[2]]);
            path.points.insert(
                sel + 1,
                [
                    (a[0] + b[0]) * 0.5,
                    (a[1] + b[1]) * 0.5,
                    (a[2] + b[2]) * 0.5,
                ],
            );
            ed.sel_point = sel + 1;
        }
        ui.same_line();
        if ui.button("Remove") {
            path.points.remove(sel);
            ed.sel_point = sel.saturating_sub(1);
        }
    }
    if path.points.is_empty() {
        return;
    }
    let sel = ed.sel_point.min(path.points.len() - 1);
    if ui.button("Up") && sel > 0 {
        path.points.swap(sel, sel - 1);
        ed.sel_point = sel - 1;
    }
    ui.same_line();
    if ui.button("Down") && sel + 1 < path.points.len() {
        path.points.swap(sel, sel + 1);
        ed.sel_point = sel + 1;
    }
    ui.separator();
    ui.child_window("##points").size([0.0, 150.0]).build(|| {
        for i in 0..path.points.len() {
            let q = path.points[i];
            let label = format!("{i}: {:.2} {:.2} {:.2}##pt{i}", q[0], q[1], q[2]);
            if ui
                .selectable_config(label)
                .selected(i == ed.sel_point)
                .build()
            {
                ed.sel_point = i;
            }
        }
    });
    let sel = ed.sel_point.min(path.points.len() - 1);
    let q = &mut path.points[sel];
    ui.text("Point (m): right / forward / up");
    ui.input_float3("##xyz", q).display_format("%.3f").build();
    ui.slider("Right##pt", -5.0, 5.0, &mut q[0]);
    ui.slider("Forward##pt", -15.0, 15.0, &mut q[1]);
    ui.slider("Up##pt", -3.0, 5.0, &mut q[2]);
}

pub(crate) fn draw_world(ui: &imgui::Ui, ed: &VehicleEditor, extra: &Extra, size: (u32, u32)) {
    if !ed.open {
        return;
    }
    let (Some(cam), Some((pos, rot))) = (extra.cam.as_ref(), extra.pose) else {
        return;
    };
    let vp = cam.view_proj(size.0 as f32 / size.1.max(1) as f32, cam.position);
    let list = ui.get_background_draw_list();
    let world = |q: [f32; 3]| -> DVec3 {
        DVec3::from(pos) + rot.transform_point3(Vec3::from(q)).as_dvec3()
    };
    if let Some(v) = extra.vehicle.as_ref() {
        if ed.show_walk {
            let green = [0.2, 1.0, 0.4, 1.0];
            let pts: Vec<Option<[f32; 2]>> = v
                .walk_points
                .iter()
                .map(|q| crate::devtools::project(&vp, cam.position, world(*q), size))
                .collect();
            for (a, b, one_way) in &v.walk_links {
                let (Some(Some(pa)), Some(Some(pb))) = (pts.get(*a as usize), pts.get(*b as usize))
                else {
                    continue;
                };
                let col = if *one_way {
                    [1.0, 0.6, 0.2, 1.0]
                } else {
                    green
                };
                list.add_line(*pa, *pb, col).thickness(2.0).build();
            }
            for (i, pt) in pts.iter().enumerate() {
                if let Some(a) = pt {
                    list.add_circle(*a, 4.0, green).filled(true).build();
                    list.add_text([a[0] + 5.0, a[1] - 5.0], green, format!("{i}"));
                }
            }
        }
        if ed.show_interior {
            for (i, src) in v.interior.iter().enumerate() {
                let c = crate::lights::interior_cfg(i);
                let q = [
                    src.pos[0] + c.shift[0],
                    src.pos[1] + c.shift[1],
                    src.pos[2] + c.shift[2],
                ];
                let Some(a) = crate::devtools::project(&vp, cam.position, world(q), size) else {
                    continue;
                };
                let m = src.color[0].max(src.color[1]).max(src.color[2]).max(1.0);
                let col = [
                    src.color[0] / m,
                    src.color[1] / m,
                    src.color[2] / m,
                    if c.off { 0.35 } else { 1.0 },
                ];
                list.add_circle(a, 7.0, col).filled(true).build();
                list.add_circle(a, 9.0, [1.0, 1.0, 1.0, col[3]])
                    .thickness(1.5)
                    .build();
                list.add_text(
                    [a[0] + 10.0, a[1] - 6.0],
                    [1.0, 1.0, 1.0, col[3]],
                    format!("#{i}"),
                );
            }
        }
    }
    if !ed.draw {
        return;
    }
    for (pi, p) in ed.paths.iter().enumerate() {
        let on = pi == ed.sel_path;
        let col = if on {
            [1.0, 0.8, 0.1, 1.0]
        } else {
            [0.6, 0.6, 0.6, 0.8]
        };
        let pts: Vec<Option<[f32; 2]>> = p
            .points
            .iter()
            .map(|q| crate::devtools::project(&vp, cam.position, world(*q), size))
            .collect();
        let n = pts.len();
        let segs = if p.closed && n > 2 {
            n
        } else {
            n.saturating_sub(1)
        };
        for i in 0..segs {
            if let (Some(a), Some(b)) = (pts[i], pts[(i + 1) % n]) {
                list.add_line(a, b, col).thickness(2.0).build();
            }
        }
        for (i, pt) in pts.iter().enumerate() {
            if let Some(a) = pt {
                let sel = on && i == ed.sel_point;
                let c = if sel { [1.0, 0.2, 0.2, 1.0] } else { col };
                list.add_circle(*a, if sel { 7.0 } else { 4.0 }, c)
                    .filled(sel)
                    .build();
            }
        }
    }
}
