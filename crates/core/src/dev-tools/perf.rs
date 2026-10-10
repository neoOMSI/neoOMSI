#![allow(unused_imports)]
use super::types::*;
use imgui::Condition;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Instant;

const MAX_REC_FRAMES: usize = 200_000;
const MAX_SPIKES: usize = 12;

#[derive(Default)]
struct SecStat {
    sum: f64,
    max: f32,
    samples: Vec<f32>,
}

struct Spike {
    frame: usize,
    at_s: f32,
    dt: f32,
    unaccounted: f32,
    top: Vec<(&'static str, f32)>,
    nested: Vec<(&'static str, f32)>,
}

struct Recording {
    started: Instant,
    dts: Vec<f32>,
    secs: BTreeMap<&'static str, SecStat>,
    spikes: Vec<Spike>,
}

fn copy_text(text: &str) -> bool {
    #[cfg(not(target_os = "android"))]
    {
        thread_local! {
            static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
        }
        CLIPBOARD.with(|c| {
            let mut c = c.borrow_mut();
            if c.is_none() {
                *c = arboard::Clipboard::new().ok();
            }
            c.as_mut().is_some_and(|cb| cb.set_text(text.to_string()).is_ok())
        })
    }
    #[cfg(target_os = "android")]
    {
        let _ = text;
        false
    }
}

pub(super) struct PerfTool {
    prev: BTreeMap<&'static str, f64>,
    prev_frames: u32,
    pub smooth: BTreeMap<&'static str, f32>,
    pub peak: BTreeMap<&'static str, f32>,
    pub frozen: bool,
    pub nested: bool,
    pub graph: Vec<f32>,
    rec: Option<Recording>,
    report: String,
    status: Option<(String, Instant)>,
}

impl PerfTool {
    pub(super) fn new() -> PerfTool {
        PerfTool {
            prev: BTreeMap::new(),
            prev_frames: 0,
            smooth: BTreeMap::new(),
            peak: BTreeMap::new(),
            frozen: false,
            nested: true,
            graph: Vec::new(),
            rec: None,
            report: String::new(),
            status: None,
        }
    }

    /// Take this frame's share of the cumulative timers (per frame, even if frames were skipped).
    pub(super) fn update(&mut self, extra: &Extra, dt_ms: f32) {
        let frames = extra.frames.wrapping_sub(self.prev_frames).max(1) as f64;
        let first = self.prev_frames == 0;
        let mut frame_secs: Vec<(&'static str, f32)> = Vec::new();
        for (k, v) in &extra.profile {
            let before = self.prev.get(k).copied().unwrap_or(*v);
            let ms = (((v - before) / frames) * 1000.0).max(0.0) as f32;
            if !first && self.rec.is_some() {
                frame_secs.push((*k, ms));
            }
            if first || self.frozen {
                continue;
            }
            let s = self.smooth.entry(k).or_insert(ms);
            *s += (ms - *s) * 0.1;
            let p = self.peak.entry(k).or_insert(0.0);
            *p = (*p * 0.995).max(ms);
        }
        self.prev = extra.profile.iter().copied().collect();
        self.prev_frames = extra.frames;
        if !first {
            if let Some(rec) = self.rec.as_mut() {
                if rec.dts.len() < MAX_REC_FRAMES {
                    let idx = rec.dts.len();
                    rec.dts.push(dt_ms);
                    for (k, ms) in &frame_secs {
                        let st = rec.secs.entry(*k).or_default();
                        st.sum += *ms as f64;
                        st.max = st.max.max(*ms);
                        // Pad skipped frames so every series has one sample per frame.
                        st.samples.resize(idx, 0.0);
                        st.samples.push(*ms);
                    }
                    let worst = rec.spikes.last().map(|s| s.dt).unwrap_or(0.0);
                    if dt_ms > 20.0 && (rec.spikes.len() < MAX_SPIKES || dt_ms > worst) {
                        frame_secs.sort_by(|a, b| b.1.total_cmp(&a.1));
                        let staged: f32 = frame_secs
                            .iter()
                            .filter(|(k, _)| !k.contains('.'))
                            .map(|p| p.1)
                            .sum();
                        let mut nested: Vec<(&'static str, f32)> = frame_secs
                            .iter()
                            .filter(|(k, v)| k.contains('.') && *k != "frame.total" && *v > 0.5)
                            .copied()
                            .collect();
                        nested.truncate(4);
                        frame_secs.retain(|(k, _)| !k.contains('.'));
                        frame_secs.truncate(4);
                        rec.spikes.push(Spike {
                            frame: idx,
                            at_s: rec.started.elapsed().as_secs_f32(),
                            dt: dt_ms,
                            unaccounted: (dt_ms - staged).max(0.0),
                            top: frame_secs,
                            nested,
                        });
                        rec.spikes.sort_by(|a, b| b.dt.total_cmp(&a.dt));
                        rec.spikes.truncate(MAX_SPIKES);
                    }
                }
            }
        }
        if !self.frozen {
            self.graph.push(dt_ms);
            if self.graph.len() > 240 {
                self.graph.remove(0);
            }
        }
    }
}

impl PerfTool {
    fn start(&mut self) {
        self.rec = Some(Recording {
            started: Instant::now(),
            dts: Vec::new(),
            secs: BTreeMap::new(),
            spikes: Vec::new(),
        });
        self.report.clear();
        self.status = None;
    }

    fn stop(&mut self, snap: &Snapshot, extra: &Extra) {
        let Some(rec) = self.rec.take() else {
            return;
        };
        self.report = build_report(&rec, snap, extra);
        let ok = copy_text(&self.report);
        let msg = if ok {
            "Report copied to clipboard"
        } else {
            "Clipboard unavailable - copy from the box below"
        };
        self.status = Some((msg.to_string(), Instant::now()));
    }
}

fn build_report(rec: &Recording, snap: &Snapshot, extra: &Extra) -> String {
    let n = rec.dts.len();
    let secs = rec.started.elapsed().as_secs_f32();
    let mut o = String::new();
    let _ = writeln!(o, "# Performance report");
    let _ = writeln!(o, "Duration {secs:.1} s, {n} frames");
    let _ = writeln!(
        o,
        "GPU: {} ({:?}), surface {}x{}, render scale {}",
        snap.adapter,
        snap.format,
        snap.surface.0,
        snap.surface.1,
        if snap.render_scale <= 0.0 {
            "auto".to_string()
        } else {
            format!("{:.2}", snap.render_scale)
        }
    );
    let _ = writeln!(
        o,
        "Settings: MSAA {}x, aniso {}x, shadows {}, SSAO {}, FXAA {}, reflections {}",
        snap.msaa, snap.anisotropy, snap.shadow_size, snap.ssao, snap.fxaa, snap.reflections
    );
    let _ = writeln!(
        o,
        "Scene: {} instances, {} meshes, {} textures, {} materials, {} lights ({} interior), {} coronas",
        snap.instances,
        snap.meshes,
        snap.textures,
        snap.materials,
        snap.lights,
        snap.interior_lights,
        snap.coronas
    );
    if let Some(t) = extra.traffic.as_ref() {
        let _ = writeln!(o, "Traffic: {} cars, {} asleep", t.cars, t.dormant);
    }
    if n == 0 {
        let _ = writeln!(o, "\nNo frames recorded.");
        return o;
    }
    let mut sorted = rec.dts.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let avg = sorted.iter().sum::<f32>() / n as f32;
    let p50 = percentile(&sorted, 0.5);
    let p95 = percentile(&sorted, 0.95);
    let p99 = percentile(&sorted, 0.99);
    let max = sorted.last().copied().unwrap_or(0.0);
    let over = |t: f32| rec.dts.iter().filter(|d| **d > t).count();
    let _ = writeln!(o, "\n## Frame time");
    let _ = writeln!(
        o,
        "avg {avg:.2} ms ({:.0} FPS), p50 {p50:.2}, p95 {p95:.2}, p99 {p99:.2}, max {max:.2} ms",
        1000.0 / avg.max(0.001)
    );
    if p99 > 0.0 {
        let _ = writeln!(o, "1% low: {:.0} FPS", 1000.0 / p99);
    }
    let _ = writeln!(
        o,
        "Frames >16.7 ms: {} ({:.1}%), >33.3 ms: {} ({:.1}%), >50 ms: {}, >100 ms: {}",
        over(16.7),
        over(16.7) as f32 / n as f32 * 100.0,
        over(33.3),
        over(33.3) as f32 / n as f32 * 100.0,
        over(50.0),
        over(100.0)
    );

    struct Row {
        name: &'static str,
        avg: f32,
        p95: f32,
        max: f32,
    }
    let mut rows: Vec<Row> = rec
        .secs
        .iter()
        .map(|(k, st)| {
            let mut v = st.samples.clone();
            v.resize(n, 0.0);
            v.sort_by(|a, b| a.total_cmp(b));
            Row {
                name: *k,
                avg: (st.sum / n as f64) as f32,
                p95: percentile(&v, 0.95),
                max: st.max,
            }
        })
        .filter(|r| r.avg > 0.005 || r.max > 0.05)
        .collect();
    rows.sort_by(|a, b| b.avg.total_cmp(&a.avg));
    let stages: f32 = rows.iter().filter(|r| !r.name.contains('.')).map(|r| r.avg).sum();
    let _ = writeln!(
        o,
        "\n## Sections (ms per frame; stages {stages:.2} ms of {avg:.2} ms, {:.2} ms outside the timers)",
        (avg - stages).max(0.0)
    );
    let _ = writeln!(o, "{:<36} {:>8} {:>8} {:>8} {:>6}", "section", "avg", "p95", "max", "%frame");
    for r in rows.iter().filter(|r| !r.name.contains('.')) {
        let _ = writeln!(
            o,
            "{:<36} {:>8.2} {:>8.2} {:>8.2} {:>5.1}%",
            r.name,
            r.avg,
            r.p95,
            r.max,
            r.avg / avg * 100.0
        );
    }
    let _ = writeln!(o, "\n## Sub-sections");
    for r in rows.iter().filter(|r| r.name.contains('.')) {
        let _ = writeln!(
            o,
            "{:<36} {:>8.2} {:>8.2} {:>8.2} {:>5.1}%",
            r.name,
            r.avg,
            r.p95,
            r.max,
            r.avg / avg * 100.0
        );
    }
    if !snap.gpu_passes.is_empty() {
        let _ = writeln!(o, "\n## GPU passes (ms per pass, average since start, passes measured)");
        let mut passes = snap.gpu_passes.clone();
        passes.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (name, ms, count) in &passes {
            let _ = writeln!(o, "{:<36} {:>8.2} {:>8}", name, ms, count);
        }
    }
    if !snap.draw_report.is_empty() {
        let _ = writeln!(o, "\n## Draws (counts: average since start; heaviest assets: last 2 s, main view)");
        for line in &snap.draw_report {
            let _ = writeln!(o, "{line}");
        }
    }
    if !rec.spikes.is_empty() {
        let _ = writeln!(
            o,
            "\n## Worst frames (dt, time into recording, unaccounted = dt minus all stages, top stages | top sub-sections)"
        );
        for s in &rec.spikes {
            let top: Vec<String> = s.top.iter().map(|(k, v)| format!("{k} {v:.1}")).collect();
            let sub: Vec<String> = s.nested.iter().map(|(k, v)| format!("{k} {v:.1}")).collect();
            let _ = writeln!(
                o,
                "frame {} @ {:.1}s: {:.1} ms, unaccounted {:.1} ms | {} | {}",
                s.frame,
                s.at_s,
                s.dt,
                s.unaccounted,
                top.join(", "),
                sub.join(", ")
            );
        }
    }
    o
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f32 * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

pub(super) fn window(ui: &imgui::Ui, open: &mut bool, tool: &mut PerfTool, snap: &Snapshot, extra: &Extra) {
    if !*open {
        return;
    }
    ui.window("Performance")
        .opened(open)
        .size([460.0, 520.0], Condition::FirstUseEver)
        .position([12.0, 32.0], Condition::FirstUseEver)
        .build(|| {
            let mut sorted = tool.graph.clone();
            sorted.sort_by(|a, b| a.total_cmp(b));
            let avg = if sorted.is_empty() {
                0.0
            } else {
                sorted.iter().sum::<f32>() / sorted.len() as f32
            };
            let p99 = percentile(&sorted, 0.99);
            let max = sorted.last().copied().unwrap_or(0.0);
            ui.text(format!("{:.0} FPS, {:.2} ms", snap.fps, snap.dt_ms));
            ui.text(format!("avg {avg:.1} ms   p99 {p99:.1} ms   max {max:.1} ms"));
            if p99 > 0.0 {
                ui.text(format!("1% low: {:.0} FPS", 1000.0 / p99));
            }
            ui.plot_lines("##perfms", &tool.graph)
                .scale_min(0.0)
                .scale_max(max.max(1.0))
                .graph_size([0.0, 64.0])
                .overlay_text(format!("max {max:.1} ms"))
                .build();
            let mut stop = false;
            if let Some(rec) = tool.rec.as_ref() {
                let c = [1.0, 0.3, 0.3, 1.0];
                ui.text_colored(
                    c,
                    format!(
                        "REC {:.1} s, {} frames",
                        rec.started.elapsed().as_secs_f32(),
                        rec.dts.len()
                    ),
                );
                ui.same_line();
                stop = ui.button("Stop + copy report");
            } else if ui.button("Start recording") {
                tool.start();
            }
            if stop {
                tool.stop(snap, extra);
            }
            if !tool.report.is_empty() && tool.rec.is_none() {
                ui.same_line();
                if ui.button("Copy report") {
                    let ok = copy_text(&tool.report);
                    let msg = if ok { "Report copied to clipboard" } else { "Clipboard unavailable" };
                    tool.status = Some((msg.to_string(), Instant::now()));
                }
            }
            if let Some((msg, at)) = tool.status.as_ref() {
                if at.elapsed().as_secs_f32() < 6.0 {
                    ui.text_colored([0.4, 1.0, 0.4, 1.0], msg);
                }
            }
            if !tool.report.is_empty() && tool.rec.is_none() {
                if ui.collapsing_header("Last report", imgui::TreeNodeFlags::empty()) {
                    ui.input_text_multiline("##perfreport", &mut tool.report, [-1.0, 220.0])
                        .read_only(true)
                        .build();
                }
            }
            ui.separator();
            ui.checkbox("Freeze", &mut tool.frozen);
            ui.same_line();
            ui.checkbox("Sub-sections", &mut tool.nested);
            ui.same_line();
            if ui.button("Reset peaks") {
                tool.peak.clear();
            }
            ui.separator();
            let mut rows: Vec<(&'static str, f32, f32)> = tool
                .smooth
                .iter()
                .filter(|(k, _)| tool.nested || !k.contains('.'))
                .map(|(k, v)| (*k, *v, tool.peak.get(k).copied().unwrap_or(0.0)))
                .filter(|r| r.1 > 0.005)
                .collect();
            rows.sort_by(|a, b| b.1.total_cmp(&a.1));
            let top = rows.first().map(|r| r.1).unwrap_or(1.0).max(0.001);
            let stages: f32 = tool
                .smooth
                .iter()
                .filter(|(k, _)| !k.contains('.'))
                .map(|(_, v)| *v)
                .sum();
            ui.text(format!(
                "Stages {stages:.2} ms of {:.2} ms (rest: {:.2} ms outside the timers)",
                snap.dt_ms,
                (snap.dt_ms - stages).max(0.0)
            ));
            ui.columns(3, "##perfcols", true);
            ui.text("Section");
            ui.next_column();
            ui.text("avg ms");
            ui.next_column();
            ui.text("peak ms");
            ui.next_column();
            ui.separator();
            for (k, v, p) in &rows {
                let hot = *v > 4.0;
                let c = if hot {
                    [1.0, 0.45, 0.3, 1.0]
                } else {
                    [1.0, 1.0, 1.0, 1.0]
                };
                ui.text_colored(c, k);
                ui.next_column();
                ui.text(format!("{v:.2}"));
                ui.same_line();
                ui.text_disabled(format!("{:.0}%", v / top * 100.0));
                ui.next_column();
                ui.text(format!("{p:.2}"));
                ui.next_column();
            }
            ui.columns(1, "##perfend", false);
            if let Some(t) = extra.traffic.as_ref() {
                ui.separator();
                ui.text(format!(
                    "Traffic: {} cars, {} asleep",
                    t.cars, t.dormant
                ));
            }
            ui.text(format!(
                "Instances {}, Meshes {}, Lights {}",
                snap.instances, snap.meshes, snap.lights
            ));
        });
}
