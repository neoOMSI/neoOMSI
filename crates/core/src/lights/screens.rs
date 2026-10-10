use super::config::{led_glow, screen_fx};
use super::frame::Frame;
use super::tuning::*;
use glam::{DVec3, Vec3};
use ::render::{LightMode, PointLight, Scene, SCREEN_CONE};
use ::simulation::VehicleInstance;

struct Panel {
    dist: f64,
    centre: DVec3,

    gate: f32,
    led: bool,
    colour: [f32; 3],

    dir: Vec3,
}

fn disabled() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| ::legacy_config::env::var_os("OMSI_NO_SCREEN_LIGHT").is_some())
}

pub(super) fn emit(f: &Frame, scene: &mut Scene, vehicles: &[&VehicleInstance]) {
    if disabled() {
        return;
    }
    let mut panels = find_panels(f, scene);
    if panels.is_empty() {
        return;
    }
    panels.sort_by(|a, b| a.dist.total_cmp(&b.dist));
    let glow = led_glow();
    let night = f.night.clamp(0.0, 1.0);
    for p in panels.into_iter().take(LED_PANELS) {
        let directional = p.dir != Vec3::ZERO;
        let out = if directional {
            p.dir.as_dvec3()
        } else {
            side_away_from_vehicle(p.centre, vehicles)
        };
        let (radius, intensity) = if p.led {
            (LED_RADIUS, LED_INTENSITY * glow * p.gate * (0.2 + 0.8 * night))
        } else {
            (SCREEN_RADIUS, SCREEN_INTENSITY * p.gate * night)
        };
        scene.lights.push(PointLight {
            position: p.centre + out * LED_OUTSET,
            radius,
            color: p.colour,
            intensity,
            direction: if directional { p.dir } else { Vec3::ZERO },
            cone: if directional { SCREEN_CONE } else { [1.0, 0.0] },
            mode: LightMode::Enhanced,
            ..Default::default()
        });
    }
}

fn find_panels(f: &Frame, scene: &Scene) -> Vec<Panel> {
    let reach = LED_RANGE + 60.0;
    let luma = scene.tex_luma.lock().ok();
    let glow = led_glow();
    let mut out = Vec::new();

    for inst in scene.instances.iter() {
        if !inst.visible || (inst.world_centre() - f.camera).length_squared() >= reach * reach {
            continue;
        }
        let has_screen = inst.materials.iter().any(|m| {
            scene
                .materials
                .get(*m)
                .is_some_and(|mat| mat.is_screen() || mat.is_led())
        });
        if !has_screen {
            continue;
        }

        let best = |led: bool| {
            let mut best = (0.0f32, SCREEN_COLOR, usize::MAX);
            for (k, m) in inst.materials.iter().enumerate() {
                let Some(mat) = scene.materials.get(*m) else { continue };
                let matches = if led {
                    mat.is_led()
                } else {
                    mat.is_screen() && !mat.is_led()
                };
                if !matches {
                    continue;
                }
                let gate = inst.slot_light.get(k).copied().unwrap_or(1.0).clamp(0.0, 1.0);
                let seen = |t: Option<usize>| t.and_then(|t| luma.as_ref().and_then(|l| l.get(&t).copied()));
                let (shown, colour) = if led {
                    (
                        seen(mat.transmap.map(|t| t.0)).map(|(_, a, _)| a).unwrap_or(0.0),
                        LED_COLOR,
                    )
                } else {
                    seen(mat.texture)
                        .map(|(c, _, rgb)| (c, rgb))
                        .unwrap_or((0.0, SCREEN_COLOR))
                };
                let fx = if led {
                    1.0
                } else if mat.is_html() {
                    screen_fx(1)
                } else {
                    screen_fx(3)
                };
                let value = gate * (shown * if led { 8.0 } else { 3.0 }).clamp(0.0, 1.0) * fx;
                if value > best.0 {
                    best = (value, colour, k);
                }
            }
            best
        };
        let led_best = if glow > 0.0 {
            best(true)
        } else {
            (0.0, SCREEN_COLOR, usize::MAX)
        };
        let (led, (gate, colour, slot)) = if led_best.0 > 0.01 {
            (true, led_best)
        } else {
            let screen_best = best(false);
            if screen_best.0 <= 0.01 {
                continue;
            }
            (false, screen_best)
        };

        let face = scene
            .meshes
            .get(inst.mesh)
            .and_then(|m| m.slot_faces.get(slot))
            .filter(|f| f.0 != Vec3::ZERO);
        let (centre, dir) = match face {
            Some((n, middle)) => (
                inst.origin + inst.transform.transform_point3(*middle).as_dvec3(),
                inst.transform.transform_vector3(*n).normalize_or_zero(),
            ),
            None => (inst.world_centre(), Vec3::ZERO),
        };
        let dist = f.dist(centre);
        if dist < LED_RANGE {
            out.push(Panel {
                dist,
                centre,
                gate,
                led,
                colour,
                dir,
            });
        }
    }
    out
}

fn side_away_from_vehicle(c: DVec3, vehicles: &[&VehicleInstance]) -> DVec3 {
    let Some(v) = vehicles
        .iter()
        .min_by(|a, b| (a.position - c).length().total_cmp(&(b.position - c).length()))
        .filter(|v| (v.position - c).length() < 20.0)
    else {
        return DVec3::ZERO;
    };
    let b = v
        .ty
        .def
        .bounding_box
        .unwrap_or([2.5, 12.0, 3.0, 0.0, 0.0, 0.0]);
    let h = v.heading.to_radians();
    let (fwd, right) = (DVec3::new(h.sin(), h.cos(), 0.0), DVec3::new(h.cos(), -h.sin(), 0.0));
    let d = c - v.position;
    let (lx, ly) = (d.dot(right) - b[3] as f64, d.dot(fwd) - b[4] as f64);
    let (ex, ey) = (
        lx.abs() - (b[0] as f64 * 0.5 - 0.5),
        ly.abs() - (b[1] as f64 * 0.5 - 0.5),
    );
    if ex <= 0.0 && ey <= 0.0 {
        DVec3::ZERO
    } else if ex > ey {
        right * lx.signum()
    } else {
        fwd * ly.signum()
    }
}
