//! Traffic routes, physical footprints and blockers on the existing GPU debug overlay.
use super::{overlays::draw_boxes, types::*, util::project};
use imgui::Condition;

pub(super) struct TrafficTool {
    pub radius: f32,
    pub selected: i32,
    paths: bool,
    boxes: bool,
    labels: bool,
    last_time: f32,
    history: Vec<[f32; 3]>,
}
impl Default for TrafficTool {
    fn default() -> Self {
        Self {
            radius: 150.0,
            selected: 0,
            paths: true,
            boxes: true,
            labels: true,
            last_time: -1.0,
            history: Vec::new(),
        }
    }
}
pub(super) fn window(
    ui: &imgui::Ui,
    open: &mut bool,
    tool: &mut TrafficTool,
    snap: &Snapshot,
    extra: &Extra,
    actions: &mut Vec<Action>,
    size: (u32, u32),
) {
    if !*open {
        return;
    }
    let Some(frame) = extra.traffic_debug.as_ref() else {
        return;
    };
    if frame.time != tool.last_time {
        tool.last_time = frame.time;
        tool.history
            .push(frame.timings.map(|s| (s * 1000.0) as f32));
        if tool.history.len() > 240 {
            tool.history.remove(0);
        }
    }
    let mut means = [0.0; 3];
    for sample in &tool.history {
        for i in 0..3 {
            means[i] += sample[i] / tool.history.len() as f32;
        }
    }
    let totals: Vec<f32> = tool.history.iter().map(|s| s.iter().sum()).collect();
    let mut sorted = totals.clone();
    sorted.sort_by(f32::total_cmp);
    let p95 = sorted
        .get(sorted.len().saturating_sub(1) * 95 / 100)
        .copied()
        .unwrap_or(0.0);
    let summary = format!(
        "Traffic AI: {}\n{} | {:.0} FPS / {:.2} ms frame | active {} dormant {} | sim {:.2}s\nAI phases ms/tick: presence {:.3}, plan {:.3}, motion/scripts {:.3}; sum p95 {:.3}\nCamera {:?}; filter {}; radius {:.0}m; {} vehicles shown (limit 64)\n",
        extra.map,
        snap.adapter,
        snap.fps,
        snap.dt_ms,
        frame.active,
        frame.dormant,
        frame.time,
        means[0],
        means[1],
        means[2],
        p95,
        extra.cam.as_ref().map(|c| c.position),
        tool.selected,
        tool.radius,
        frame.cars.len()
    );
    ui.window("Traffic AI")
        .opened(open)
        .size([540.0, 460.0], Condition::FirstUseEver)
        .build(|| {
            ui.text(format!(
                "{:.0} FPS | {:.2} ms frame | {} active, {} dormant",
                snap.fps, snap.dt_ms, frame.active, frame.dormant
            ));
            ui.text(format!(
                "AI ms/tick: presence {:.3}, plan {:.3}, motion/scripts {:.3}",
                means[0], means[1], means[2]
            ));
            ui.plot_lines("##traffic_ms", &totals)
                .graph_size([0.0, 55.0])
                .overlay_text(format!("phase sum p95 {p95:.3} ms"))
                .build();
            ui.text_disabled(
                "FPS includes rendering; AI phase times exclude GPU and debug drawing.",
            );
            ui.slider("Radius (m)", 20.0, 300.0, &mut tool.radius);
            ui.input_int("Vehicle ID (0 = nearby)", &mut tool.selected)
                .build();
            ui.checkbox("Routes", &mut tool.paths);
            ui.same_line();
            ui.checkbox("Physical boxes", &mut tool.boxes);
            ui.same_line();
            ui.checkbox("Labels", &mut tool.labels);
            ui.text_disabled(
                "Green route, yellow lane change, red blocker/hold, cyan stop origin.",
            );
            if ui.button("Copy diagnostic report") {
                let mut report = summary.clone();
                report.push_str(&format!("Viewport {:?}, render scale {}, MSAA {}, SSAO {}, shadows {}; instances {}, meshes {}, textures {}; overlay routes {} boxes {} labels {}\n",
                    snap.surface, snap.render_scale, snap.msaa, snap.ssao, snap.shadow_size,
                    snap.instances, snap.meshes, snap.textures, tool.paths, tool.boxes, tool.labels));
                for car in &frame.cars {
                    report.push_str(&format!(
                        "#{} {} {:.1} km/h at {:?}: {}\n  blocker {:?}; stop {:?}\n",
                        car.id,
                        car.model,
                        car.speed * 3.6,
                        car.position,
                        car.detail,
                        car.blocker.map(|b| b.0),
                        car.stop
                    ));
                }
                actions.push(Action::CopyTrafficReport(report));
            }
            ui.same_line();
            if ui.button("Reset timing history") {
                tool.history.clear();
            }
            ui.separator();
            for car in &frame.cars {
                if ui
                    .selectable_config(format!(
                        "#{} {} {:.1} km/h",
                        car.id,
                        car.model,
                        car.speed * 3.6
                    ))
                    .selected(tool.selected as u64 == car.id)
                    .build()
                {
                    tool.selected = i32::try_from(car.id).unwrap_or(0);
                }
                ui.text_wrapped(&car.detail);
                if let Some((stop, _, distance)) = car.stop {
                    ui.text(format!("Stop #{stop}: target origin {distance:.1} m ahead"));
                }
            }
        });
    let Some(cam) = extra.cam.as_ref() else {
        return;
    };
    let vp = cam.view_proj(size.0 as f32 / size.1.max(1) as f32, cam.position);
    let list = ui.get_background_draw_list();
    let project = |p| project(&vp, cam.position, p, size);
    for car in &frame.cars {
        let colour = if car.changing {
            [1.0, 0.8, 0.15, 0.9]
        } else {
            [0.15, 1.0, 0.3, 0.9]
        };
        if tool.paths {
            for pair in car.path.windows(2) {
                if let (Some(a), Some(b)) = (project(pair[0]), project(pair[1])) {
                    list.add_line(a, b, colour).thickness(2.0).build();
                    list.add_circle(b, 2.0, colour).filled(true).build();
                }
            }
        }
        if tool.boxes {
            draw_boxes(ui, cam, size, &car.boxes, Some([0.2, 0.75, 1.0, 0.75]));
        }
        let origin = project(car.position + glam::DVec3::Z * 2.5);
        if tool.labels {
            if let Some(p) = origin {
                list.add_text(p, [1.0, 1.0, 1.0, 1.0], &car.label);
            }
        }
        if let Some((id, target)) = car.blocker {
            if let (Some(a), Some(b)) = (origin, project(target)) {
                list.add_line(a, b, [1.0, 0.2, 0.1, 0.9])
                    .thickness(2.0)
                    .build();
                list.add_text(b, [1.0, 0.3, 0.2, 1.0], format!("blocker #{id}"));
            }
        }
        if let Some(p) = car.constraint.and_then(project) {
            list.add_circle(p, 6.0, [1.0, 0.1, 0.1, 1.0])
                .thickness(2.0)
                .build();
        }
        if let Some((id, point, _)) = car.stop {
            if let Some(p) = project(point) {
                list.add_circle(p, 7.0, [0.0, 1.0, 1.0, 1.0])
                    .thickness(2.0)
                    .build();
                list.add_text(p, [0.0, 1.0, 1.0, 1.0], format!("stop #{id}"));
            }
        }
    }
}
