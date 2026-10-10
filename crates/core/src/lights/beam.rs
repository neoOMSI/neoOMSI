use super::config::{half_cos, LightSettings};
use super::fader::Faders;
use super::sprites::glow_texture_id;
use super::tuning::*;
use glam::{DVec3, Mat4, Vec3};
use ::render::{Corona, LightMode, PointLight};
use ::simulation::VehicleInstance;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BeamKind {
    Dipped,
    Main,
}

const MAIN_MIN_RANGE: f32 = 80.0;
const MAIN_RANGE_RATIO: f32 = 1.25;
const MAX_DROP: f32 = 0.03;
pub(super) const FOG_DROP: f32 = 0.025;

pub(super) fn headlight_radius(range: f32) -> f32 {
    range.max(6.0)
}

pub(super) fn headlight_core(range: f32) -> f32 {
    headlight_radius(range) / 30.0
}

pub(super) fn spot_reach(range: f32, low: f32) -> f32 {
    range.clamp(0.5, low).max(range * low / 100.0).min(low * 5.0)
}

pub(super) fn short_range_gain(range: f32) -> f32 {
    if range >= 10.0 {
        1.0
    } else {
        let r = range.max(0.5) / 10.0;
        r * r
    }
}

pub(super) fn main_beam_code(range: f32) -> f32 {
    let k = (spot_reach(range, 60.0) / 60.0).max(1.0);
    -(k * k)
}

pub(super) fn aim_drop(mount_height: f32) -> f32 {
    if mount_height < 0.8 {
        0.010
    } else if mount_height <= 1.0 {
        0.012
    } else {
        0.015
    }
}

pub(super) fn aimed(dir: Vec3, drop: f32) -> Vec3 {
    let h = dir.truncate().length();
    if h < 1e-4 {
        return dir;
    }
    let slope = (dir.z / h).min(-drop);
    Vec3::new(dir.x / h, dir.y / h, slope).normalize_or_zero()
}

fn aimed_limited(dir: Vec3, drop: f32) -> Vec3 {
    let a = aimed(dir, drop);
    let h = a.truncate().length().max(1e-4);
    Vec3::new(a.x / h, a.y / h, (a.z / h).max(-MAX_DROP)).normalize_or_zero()
}

pub(super) fn facing(dir: Vec3) -> f32 {
    if dir.y > 0.3 {
        1.0
    } else if dir.y < -0.3 {
        -1.0
    } else {
        0.0
    }
}

pub(super) fn spot_face(lamp: Option<f32>, edge: Option<f32>, apex_y: f32, dir: f32) -> Option<f32> {
    let fwd = |y: f32| y * dir;
    let lamp = lamp.filter(|l| fwd(*l) > fwd(apex_y));
    let face = match (lamp, edge) {
        (Some(l), Some(e)) => fwd(l).min(fwd(e)),
        (Some(l), None) => fwd(l),
        (None, Some(e)) => fwd(e).min(fwd(apex_y) + 1.5),
        (None, None) => return None,
    };
    Some(face * dir).filter(|f| fwd(*f) > fwd(apex_y))
}

pub(super) fn ai_spotlight(lamps: &[[f32; 3]]) -> Option<[f32; 12]> {
    let nose = lamps.iter().map(|l| l[1]).reduce(f32::max)?;
    let front: Vec<&[f32; 3]> = lamps.iter().filter(|l| nose - l[1] < 0.4).collect();
    let z = front.iter().map(|l| l[2]).sum::<f32>() / front.len() as f32;
    Some([0.0, nose, z, 0.0, 1.0, -0.05, 255.0, 245.0, 225.0, 40.0, 30.0, 70.0])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Profile {
    Enviro400Mmc,
    Renown,
    LedHalo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LampRole {
    Beam,
    Fog,
    Cornering,
    Drl,
}

impl LampRole {
    fn gain(self) -> f32 {
        match self {
            Self::Beam => 1.0,
            Self::Fog => 0.6,
            Self::Cornering => 0.35,
            Self::Drl => 0.08,
        }
    }
}

impl Profile {
    pub(super) fn of_path(path: &str) -> Option<Self> {
        let path = path.to_ascii_lowercase();
        if path.contains("studio polygon 400mmc") {
            Some(Self::Enviro400Mmc)
        } else if path.contains("studio polygon renown") {
            Some(Self::Renown)
        } else {
            None
        }
    }

    pub(super) fn of_model(path: &str, spots: usize, has_var: &dyn Fn(&str) -> bool) -> Option<Self> {
        Self::of_path(path).or_else(|| {
            (spots == 11 && has_var("lights_fern_led") && has_var("lights_abbl_halo"))
                .then_some(Self::LedHalo)
        })
    }

    pub(super) fn kind(self, entry: usize) -> BeamKind {
        let is_main = match self {
            Self::Enviro400Mmc => entry == 0,
            Self::Renown => entry == 2,
            Self::LedHalo => entry == 0 || entry == 6,
        };
        if is_main {
            BeamKind::Main
        } else {
            BeamKind::Dipped
        }
    }

    pub(super) fn role(self, entry: usize) -> LampRole {
        match (self, entry) {
            (Self::LedHalo, 1 | 7) => LampRole::Fog,
            (Self::LedHalo, 2 | 3 | 8 | 9) => LampRole::Cornering,
            (Self::LedHalo, 5) => LampRole::Drl,
            _ => LampRole::Beam,
        }
    }

    pub(super) fn side(self, entry: usize) -> Option<f32> {
        match (self, entry) {
            (Self::LedHalo, 2 | 8) => Some(1.0),
            (Self::LedHalo, 3 | 9) => Some(-1.0),
            _ => None,
        }
    }

    pub(super) fn partner(self, entry: usize) -> Option<usize> {
        match (self, entry) {
            (Self::LedHalo, 0) => Some(4),
            (Self::LedHalo, 6) => Some(10),
            (Self::LedHalo, _) => None,
            _ => Some(1),
        }
    }

    pub(super) fn visual_only(self, entry: usize) -> bool {
        self == Self::Enviro400Mmc && entry == 3
    }

    pub(super) fn lamp_variables(self, entry: usize) -> &'static [&'static str] {
        match (self, entry) {
            (Self::Enviro400Mmc, 0) => &["lights_highbeam"],
            (Self::Enviro400Mmc, 1) => &["lights_mainbeam"],
            (Self::Renown, 0) => &["lights_lowbeam"],
            (Self::Renown, 1) => &["lights_mainbeam"],
            (Self::Renown, 2) => &["lights_highbeam"],
            (Self::LedHalo, 0) => &["lights_fern_LED"],
            (Self::LedHalo, 1 | 7) => &["light_nebelfront_R", "light_nebelfront_L"],
            (Self::LedHalo, 2 | 3 | 4 | 5) => &["lights_abbl_LED"],
            (Self::LedHalo, 6) => &["lights_fern_halo"],
            (Self::LedHalo, 8 | 9 | 10) => &["lights_abbl_halo"],
            _ => &[],
        }
    }
}

pub(super) fn classify(ranges: &[f32], selected: usize) -> BeamKind {
    let Some(&range) = ranges.get(selected) else {
        return BeamKind::Dipped;
    };
    if ranges.len() < 2 {
        return BeamKind::Dipped;
    }
    let shortest = ranges.iter().copied().fold(f32::MAX, f32::min);
    if range >= MAIN_MIN_RANGE && range > shortest * MAIN_RANGE_RATIO {
        BeamKind::Main
    } else {
        BeamKind::Dipped
    }
}

struct Headlamp<'a> {
    vals: [f32; 12],
    kind: BeamKind,
    body: Mat4,
    origin: DVec3,
    lamps: &'a [[f32; 3]],
    sources: &'a [[f32; 3]],
    bounding_box: Option<[f32; 6]>,
    night: f32,
    level: f32,
    gain: f32,
    range_mul: f32,
    role: LampRole,
    side: Option<f32>,
}

pub(super) fn headlamps(
    cfg: &LightSettings,
    faders: &mut Faders,
    v: &VehicleInstance,
    night: f32,
    lights: &mut Vec<PointLight>,
    coronas: &mut Vec<Corona>,
) {
    if !cfg.low.on && !cfg.high.on {
        return;
    }
    let ty = &v.ty;
    let ai_on = v.ai_lights;
    // `-1` is OMSI's explicit "no road beam"; it must not turn into spotlight zero.
    let selected = v.var("Spot_Select").filter(|s| *s >= 0.0);
    let sel = selected.or(ai_on.then_some(0.0));

    let lamp_sprites = || {
        ty.model.meshes.iter().flat_map(|m| {
            m.light_enh
                .iter()
                .map(|l| (l.pos, l.variable.as_str()))
                .chain(m.light_enh_2.iter().map(|l| (l.pos, l.variable.as_str())))
        })
    };
    let lamps: Vec<[f32; 3]> = lamp_sprites().map(|(p, _)| p).collect();

    let mut spots: Vec<[f32; 12]> = ty.model.spotlights.iter().map(|s| s.values).collect();
    if spots.is_empty() && ai_on {
        spots.extend(ai_spotlight(&lamps));
    }
    if spots.is_empty() {
        return;
    }

    let lit = sel.map(|s| {
        let i = s as usize;
        if i < spots.len() {
            i
        } else if ai_on {
            0
        } else {
            usize::MAX
        }
    });
    let ranges: Vec<f32> = spots.iter().map(|s| s[9]).collect();
    let profile = Profile::of_model(&v.ty.def.path.to_string_lossy(), spots.len(), &|name| {
        lamp_sprites().any(|(_, var)| var.eq_ignore_ascii_case(name))
    });
    let kinds: Vec<BeamKind> = (0..spots.len())
        .map(|i| profile.map_or_else(|| classify(&ranges, i), |p| p.kind(i)))
        .collect();

    // The shortest dipped entry burns together with a full beam.
    let partner = if let Some(p) = profile {
        lit.and_then(|i| p.partner(i)).filter(|&i| i < spots.len())
    } else {
        (0..spots.len())
            .filter(|&i| {
                kinds[i] == BeamKind::Dipped && !profile.is_some_and(|p| p.visual_only(i))
            })
            .min_by(|&a, &b| ranges[a].total_cmp(&ranges[b]))
    };
    let main_lit = lit.is_some_and(|i| i < spots.len() && kinds[i] == BeamKind::Main);
    let visual_only = matches!((profile, lit), (Some(p), Some(i)) if p.visual_only(i));

    let owner = super::owner_key(v);
    for (i, vals) in spots.iter().enumerate() {
        let on = !visual_only && (lit == Some(i) || (main_lit && partner == Some(i)));
        let level = faders.level(owner, i as u32, if on { 1.0 } else { 0.0 }, LAMP_RISE, LAMP_FALL);
        if level < 0.01 {
            continue;
        }
        let kind = kinds[i];
        let beam_cfg = match kind {
            BeamKind::Dipped => cfg.low,
            BeamKind::Main => cfg.high,
        };
        if !beam_cfg.on {
            continue;
        }
        // Profiled models name their real lamp effects: bind the beam to those positions.
        let role = profile.map_or(LampRole::Beam, |p| p.role(i));
        let side = profile.and_then(|p| p.side(i));
        let vars = profile.map_or(&[][..], |p| p.lamp_variables(i));
        let sources: Vec<[f32; 3]> = lamp_sprites()
            .filter(|(_, v)| vars.iter().any(|n| v.eq_ignore_ascii_case(n)))
            .filter(|(p, _)| side.map_or(true, |s| p[0] * s > 0.0))
            .map(|(p, _)| p)
            .collect();
        emit_headlamp(
            cfg,
            &Headlamp {
                vals: *vals,
                kind,
                body: v.body_rotation(),
                origin: v.position,
                lamps: &lamps,
                sources: &sources,
                bounding_box: ty.def.bounding_box,
                night,
                level,
                gain: beam_cfg.gain,
                range_mul: beam_cfg.range,
                role,
                side,
            },
            lights,
            coronas,
        );
    }
}

fn emit_headlamp(
    cfg: &LightSettings,
    h: &Headlamp,
    lights: &mut Vec<PointLight>,
    coronas: &mut Vec<Corona>,
) {
    let vals = h.vals;
    let raw_dir = Vec3::new(vals[3], vals[4], vals[5]).normalize_or_zero();
    if raw_dir == Vec3::ZERO {
        return;
    }
    let range = vals[9] * h.range_mul.max(0.05);
    let dir_y = facing(raw_dir);
    let mut apex = Vec3::new(vals[0], vals[1], vals[2]);
    if dir_y != 0.0 {
        let nose = h
            .lamps
            .iter()
            .map(|l| l[1] * dir_y)
            .reduce(f32::max)
            .map(|m| m * dir_y);
        let edge = h.bounding_box.map(|bb| bb[4] + dir_y * bb[1] * 0.5);
        if let Some(face) = spot_face(nose, edge, apex.y, dir_y) {
            apex.y = face + dir_y * 0.05;
        }
    }
    let axis_at = |mount_z: f32| -> Vec3 {
        if dir_y != 0.0 {
            aimed_limited(raw_dir, aim_drop(mount_z))
        } else {
            raw_dir
        }
    };

    // Lamp spacing: the entry's own offset, else the sprites at the same height, else the
    // body width.
    let half_width = h.bounding_box.map_or(1.25, |bb| (bb[0] * 0.5).min(1.25));
    let face_x: Vec<f32> = h
        .lamps
        .iter()
        .filter(|l| dir_y != 0.0 && (l[1] - apex.y).abs() < 0.35 && l[0].abs() > 0.1)
        .map(|l| l[0].abs())
        .collect();
    let spread = if vals[0].abs() > 0.1 {
        vals[0].abs()
    } else if !face_x.is_empty() {
        face_x.iter().sum::<f32>() / face_x.len() as f32
    } else {
        half_width * 0.6
    }
        .min(half_width.max(0.3));
    let right = h.body.transform_vector3(Vec3::X).normalize_or_zero();
    let base = h.body.transform_point3(Vec3::new(0.0, apex.y, apex.z));

    let outer = vals[10].max(vals[11]).max(match h.kind {
        BeamKind::Dipped => 80.0,
        BeamKind::Main => 50.0,
    });
    let inner_frac = if dir_y < 0.0 { 0.6 } else { 0.3 };
    let cone = [half_cos(outer * inner_frac), half_cos(outer)];
    let reach_vanilla = spot_reach(range, 45.0);
    let radius = match (h.role, h.kind) {
        (LampRole::Fog, _) => range.clamp(6.0, 45.0),
        (LampRole::Cornering, _) => range.clamp(6.0, 25.0),
        (LampRole::Drl, _) => range.clamp(3.0, 8.0),
        (LampRole::Beam, BeamKind::Dipped) => spot_reach(range, 60.0).max(60.0),
        (LampRole::Beam, BeamKind::Main) => range.clamp(MAIN_MIN_RANGE, 300.0),
    };
    let role_gain = h.role.gain();
    let short = short_range_gain(range);
    let code = match h.kind {
        BeamKind::Dipped => CODE_DIPPED,
        BeamKind::Main => CODE_MAIN,
    };
    let color = [vals[6] / 255.0, vals[7] / 255.0, vals[8] / 255.0];

    // A dipped beam also lights the bumper area right in front of the lamps.
    if h.kind == BeamKind::Dipped && h.role == LampRole::Beam {
        let forward = h.body.transform_vector3(axis_at(apex.z)).normalize_or_zero();
        lights.push(PointLight {
            position: h.origin + (base + forward * 0.35).as_dvec3(),
            radius: 1.6,
            color,
            intensity: 0.35 * (0.4 + 0.6 * h.night) * h.level * h.gain.min(1.5),
            core: 0.15,
            mode: LightMode::Enhanced,
            ..Default::default()
        });
    }

    let sources: Vec<(Vec3, f32)> = if h.sources.is_empty() {
        [-1.0f32, 1.0]
            .into_iter()
            .filter(|side| h.side.map_or(true, |s| s == *side))
            .map(|side| (base + right * spread * side, apex.z))
            .collect()
    } else {
        h.sources
            .iter()
            .map(|s| (h.body.transform_point3(Vec3::from(*s)), s[2]))
            .collect()
    };
    for (at, mount_z) in sources {
        let d = h.body.transform_vector3(axis_at(mount_z)).normalize_or_zero();
        let lamp = PointLight {
            position: h.origin + at.as_dvec3(),
            color,
            direction: d,
            cone,
            ..Default::default()
        };
        if h.kind == BeamKind::Dipped && h.role == LampRole::Beam && h.sources.is_empty() {
            // glare sprite on the nearest lamp sprite
            let glare_at = h
                .lamps
                .iter()
                .map(|l| h.body.transform_point3(Vec3::from(*l)))
                .filter(|p| (*p - at).length() < 0.6)
                .min_by(|a, b| (*a - at).length().total_cmp(&(*b - at).length()))
                .unwrap_or(at + d * 0.05);
            coronas.push(Corona {
                position: h.origin + glare_at.as_dvec3(),
                size: 0.15,
                color,
                brightness: h.level.min(1.0),
                direction: Vec3::ZERO,
                cone_cos: -1.0,
                rotating: 2,
                texture: glow_texture_id(),
                ..Default::default()
            });
        }
        // vanilla renderer's lamp
        lights.push(PointLight {
            radius: reach_vanilla,
            intensity: cfg.vanilla * 0.5 * (0.3 + 0.7 * h.night) * short * role_gain * h.level * h.gain,
            mode: LightMode::Vanilla,
            ..lamp
        });
        // enhanced renderer's lamp
        lights.push(PointLight {
            radius,
            intensity: cfg.headlight * 0.5 * short * role_gain * h.level * h.gain,
            core: 1.0,
            beam: code,
            mode: LightMode::Enhanced,
            ..lamp
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_beam_range_is_not_capped_to_dipped_beam_distance() {
        assert_eq!(headlight_radius(125.0), 125.0);
        assert!((headlight_core(125.0) - 125.0 / 30.0).abs() < 1e-5);
    }

    #[test]
    fn sp400_keeps_drls_visual_only() {
        let p = Profile::of_path(r"Vehicles\[SP] Studio Polygon 400MMC\Model\E400MMC.bus").unwrap();
        assert_eq!(p, Profile::Enviro400Mmc);
        assert_eq!(p.kind(0), BeamKind::Main);
        assert_eq!(p.kind(1), BeamKind::Dipped);
        assert!(p.visual_only(3));
        assert_eq!(p.lamp_variables(0), ["lights_highbeam"]);
        assert!(p.lamp_variables(3).is_empty());
    }

    #[test]
    fn renown_keeps_only_its_third_entry_as_full_beam() {
        let p = Profile::of_path(r"Vehicles\[SP] Studio Polygon Renown\Model\Renown.bus").unwrap();
        assert_eq!(p.kind(0), BeamKind::Dipped);
        assert_eq!(p.kind(1), BeamKind::Dipped);
        assert_eq!(p.kind(2), BeamKind::Main);
        assert_eq!(p.lamp_variables(1), ["lights_mainbeam"]);
    }

    #[test]
    fn led_halo_profile_roles() {
        let p = Profile::of_model("x.bus", 11, &|n| n == "lights_fern_led" || n == "lights_abbl_halo").unwrap();
        assert_eq!(p, Profile::LedHalo);
        assert_eq!(p.kind(0), BeamKind::Main);
        assert_eq!(p.kind(4), BeamKind::Dipped);
        assert_eq!(p.role(5), LampRole::Drl);
        assert_eq!(p.role(2), LampRole::Cornering);
        assert_eq!(p.side(3), Some(-1.0));
        assert_eq!(p.partner(6), Some(10));
    }

    #[test]
    fn generic_classifier() {
        assert_eq!(Profile::of_path(r"Vehicles\MAN_NL202\NL202.bus"), None);
        assert_eq!(classify(&[40.0, 80.0], 1), BeamKind::Main);
        assert_eq!(classify(&[40.0, 80.0], 0), BeamKind::Dipped);
    }

    #[test]
    fn aim_never_rises_above_drop() {
        let d = aimed(Vec3::new(0.0, 1.0, 0.2), 0.01);
        assert!(d.z < 0.0);
    }
}
