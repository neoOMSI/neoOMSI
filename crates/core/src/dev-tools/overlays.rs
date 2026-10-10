#![allow(unused_imports)]
use super::types::*;
use super::util::*;
use ::simulation::collision::Obb;

pub(super) fn draw_boxes(
    ui: &imgui::Ui,
    cam: &::render::Camera,
    size: (u32, u32),
    boxes: &[Obb],
    over: Option<[f32; 4]>,
) {
    let list = ui.get_background_draw_list();
    draw_boxes_on(&list, cam, size, boxes, over);
}

/// Use the caller's draw list when boxes are part of a larger overlay. ImGui allows
/// only one live background DrawListMut; acquiring it again would panic.
pub(super) fn draw_boxes_on(
    list: &imgui::DrawListMut<'_>,
    cam: &::render::Camera,
    size: (u32, u32),
    boxes: &[Obb],
    over: Option<[f32; 4]>,
) {
    let vp = cam.view_proj(size.0 as f32 / size.1.max(1) as f32, cam.position);
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

pub(super) fn draw_beams(
    ui: &imgui::Ui,
    cam: &::render::Camera,
    size: (u32, u32),
    beams: &[BeamMark],
) {
    let vp = cam.view_proj(size.0 as f32 / size.1.max(1) as f32, cam.position);
    let list = ui.get_background_draw_list();
    for b in beams {
        // (yellow: the fog cone's start, cyan: the headlight's start; the ray tapers off along 12 m of its axis)
        let col = if let Some(t) = b.tint {
            [t[0], t[1], t[2], 1.0]
        } else if b.cone {
            [1.0, 0.9, 0.1, 1.0]
        } else {
            [0.1, 0.9, 1.0, 1.0]
        };
        let p = glam::DVec3::from(b.pos);
        let d = glam::Vec3::from(b.dir).normalize_or_zero().as_dvec3();
        let Some(pa) = project(&vp, cam.position, p, size) else {
            continue;
        };
        // a long ray that tapers off: segments get thinner and fainter with distance
        const SEGS: usize = 16;
        const LEN: f64 = 12.0;
        let mut prev = pa;
        for i in 1..=SEGS {
            let t = i as f64 / SEGS as f64;
            let Some(next) = project(&vp, cam.position, p + d * (LEN * t * t), size) else {
                break;
            };
            let f = (1.0 - t) as f32;
            let c = [col[0], col[1], col[2], 0.15 + 0.85 * f * f];
            list.add_line(prev, next, c)
                .thickness(0.8 + 1.7 * f)
                .build();
            prev = next;
        }
        list.add_circle(pa, 6.0, col).thickness(2.0).build();
        list.add_line([pa[0] - 9.0, pa[1]], [pa[0] + 9.0, pa[1]], col)
            .build();
        list.add_line([pa[0], pa[1] - 9.0], [pa[0], pa[1] + 9.0], col)
            .build();
    }
}

pub(super) fn draw_cameras(
    ui: &imgui::Ui,
    cam: &::render::Camera,
    size: (u32, u32),
    infos: &[crate::camera_tool::CamInfo],
) {
    let vp = cam.view_proj(size.0 as f32 / size.1.max(1) as f32, cam.position);
    let list = ui.get_background_draw_list();
    for (i, inf) in infos.iter().enumerate() {
        let col = if !inf.seen {
            [0.6, 0.6, 0.6, 1.0]
        } else if inf.direct {
            [1.0, 0.2, 0.9, 1.0]
        } else {
            [0.1, 0.9, 1.0, 1.0]
        };
        let p = glam::DVec3::from(inf.eye);
        let d = glam::DVec3::from(inf.dir).normalize_or_zero();
        let Some(pa) = project(&vp, cam.position, p, size) else {
            continue;
        };
        const SEGS: usize = 16;
        const LEN: f64 = 12.0;
        let mut prev = pa;
        for s in 1..=SEGS {
            let t = s as f64 / SEGS as f64;
            let Some(next) = project(&vp, cam.position, p + d * (LEN * t * t), size) else {
                break;
            };
            let f = (1.0 - t) as f32;
            let c = [col[0], col[1], col[2], 0.15 + 0.85 * f * f];
            list.add_line(prev, next, c).thickness(0.8 + 1.7 * f).build();
            prev = next;
        }
        if crate::camera_tool::overlay_view() {
            // (the ray a mirror draws: follows the viewer's eye, so it moves with the camera)
            let v = glam::DVec3::from(inf.view).normalize_or_zero();
            if let Some(pv) = project(&vp, cam.position, p + v * 6.0, size) {
                list.add_line(pa, pv, [1.0, 0.85, 0.1, 0.9]).thickness(1.0).build();
            }
        }
        list.add_circle(pa, 6.0, col).thickness(2.0).build();
        list.add_line([pa[0] - 9.0, pa[1]], [pa[0] + 9.0, pa[1]], col)
            .build();
        list.add_line([pa[0], pa[1] - 9.0], [pa[0], pa[1] + 9.0], col)
            .build();
        let part = if inf.part == 0 {
            String::new()
        } else {
            format!(" p{}", inf.part)
        };
        list.add_text([pa[0] + 9.0, pa[1] - 16.0], col, format!("#{i}{part}"));
    }
}

pub(super) fn draw_doors(
    ui: &imgui::Ui,
    cam: &::render::Camera,
    size: (u32, u32),
    doors: &[DoorDbg],
) {
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
