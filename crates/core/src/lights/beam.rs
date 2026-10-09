use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BeamKind {
    Dipped,
    Main,
}

const MAIN_MIN_RANGE: f32 = 80.0;
const MAIN_RANGE_RATIO: f32 = 1.25;
pub(super) const FOG_DROP: f32 = 0.025;

/// The Studio Polygon 400MMC's fourth classic spotlight is its DRL.  OMSI selects it for the
/// visible daytime-running lamps, but it must not become an environmental road light.
///
/// This is deliberately a content-specific compatibility profile.  Spotlight indices and
/// ranges are arbitrary in OMSI content, so applying this rule to every vehicle suppresses
/// legitimate dipped beams on other buses.
fn is_studio_polygon_400mmc(v: &VehicleInstance) -> bool {
    v.ty.def
        .path
        .to_string_lossy()
        .to_ascii_lowercase()
        .contains("studio polygon 400mmc")
}

fn is_studio_polygon_400mmc_drl(v: &VehicleInstance, selected: Option<usize>) -> bool {
    is_studio_polygon_400mmc(v) && selected == Some(3)
}

/// The Renown's three road-spot entries are low, dipped and full in that order.  In
/// particular, its 80 m dipped entry must not be promoted to full beam merely because it is
/// longer than the 40 m low-light entry.
fn is_studio_polygon_renown(v: &VehicleInstance) -> bool {
    v.ty.def
        .path
        .to_string_lossy()
        .to_ascii_lowercase()
        .contains("studio polygon renown")
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

pub(super) fn beam_code(kind: BeamKind, range: f32) -> f32 {
    match kind {
        BeamKind::Dipped => 100.0,
        BeamKind::Main => {
            let k = (spot_reach(range, 60.0) / 60.0).max(1.0);
            -(k * k)
        }
    }
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
    let h = (dir.x * dir.x + dir.y * dir.y).sqrt();
    if h < 1e-4 {
        return dir;
    }
    let slope = (dir.z / h).min(-drop);
    Vec3::new(dir.x / h, dir.y / h, slope).normalize_or_zero()
}

pub(super) struct Headlamp<'a> {
    pub vals: [f32; 12],
    pub kind: BeamKind,
    pub body: glam::Mat4,
    pub origin: DVec3,
    pub lamps: &'a [[f32; 3]],
    /// The physical lamp positions for this particular road-light selection.  Most OMSI
    /// vehicles do not provide an association, so they retain the symmetric fallback.
    pub sources: &'a [[f32; 3]],
    pub bounding_box: Option<[f32; 6]>,
    pub night: f32,
    pub level: f32,
    pub gain: f32,
    pub range_mul: f32,
}

pub(super) fn headlamps(
    v: &VehicleInstance,
    night: f32,
    lights: &mut Vec<PointLight>,
    coronas: &mut Vec<Corona>,
) {
    let cfg = settings();
    if !cfg.low.on && !cfg.high.on {
        return;
    }
    let ty = &v.ty;
    let ai_on = v.ai_lights;
    // `-1` is OMSI's explicit "no road beam" value.  Do not turn it into
    // spotlight zero for player vehicles.
    let selected = v.var("Spot_Select").filter(|s| *s >= 0.0);
    let sel = selected.or(ai_on.then_some(0.0));
    let lamps: Vec<[f32; 3]> = ty
        .model
        .meshes
        .iter()
        .flat_map(|m| {
            m.light_enh
                .iter()
                .map(|l| l.pos)
                .chain(m.light_enh_2.iter().map(|l| l.pos))
        })
        .collect();
    let mut spots: Vec<[f32; 12]> = ty.model.spotlights.iter().map(|s| s.values).collect();
    if spots.is_empty() && ai_on {
        if let Some(vals) = ai_spotlight(&lamps) {
            spots.push(vals);
        }
    }
    if spots.is_empty() {
        return;
    }
    let lit = sel.filter(|s| *s >= 0.0).map(|s| {
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
    let sp400 = is_studio_polygon_400mmc(v);
    let renown = is_studio_polygon_renown(v);
    let kinds: Vec<BeamKind> = (0..spots.len())
        .map(|i| {
            // SP400: 0 is full beam, 1 is dipped, 2 is fog and 3 is DRL.  Its ranges are
            // not ordered like the generic OMSI convention, so keep that mapping local.
            if sp400 {
                if i == 0 {
                    BeamKind::Main
                } else {
                    BeamKind::Dipped
                }
            } else if renown {
                if i == 2 {
                    BeamKind::Main
                } else {
                    BeamKind::Dipped
                }
            } else {
                classify(&ranges, i)
            }
        })
        .collect();

    let partner = (0..spots.len())
        .filter(|&i| kinds[i] == BeamKind::Dipped && (!sp400 || i != 3))
        .min_by(|&a, &b| ranges[a].total_cmp(&ranges[b]));
    let main_lit = lit.is_some_and(|i| i < spots.len() && kinds[i] == BeamKind::Main);
    let sp400_drl = is_studio_polygon_400mmc_drl(v, lit);
    let key = key_of(v);
    for (i, vals) in spots.iter().enumerate() {
        // The SP400 exposes an extra road-light selection while its visual DRLs are on.
        // Its middle DRL entities are rendered by `vehicle_lights`; they must not create
        // the generated environmental beam below.
        let on = !sp400_drl && (lit == Some(i) || (main_lit && partner == Some(i)));
        let level = lamp_level(
            key,
            i as u32,
            if on { 1.0 } else { 0.0 },
            LAMP_RISE,
            LAMP_FALL,
        );
        if level < 0.01 {
            continue;
        }
        let kind = kinds[i];
        let bc = match kind {
            BeamKind::Dipped => cfg.low,
            BeamKind::Main => cfg.high,
        };
        if !bc.on {
            continue;
        }
        // These Studio Polygon models centre their classic road-spot definitions, but name
        // the real lamp effects.  Bind the generated road beam to those real positions.
        // This also avoids using the generic symmetric fallback for the Renown's compact
        // inner/outer lamp cluster.
        let source_variable = if sp400 {
            match i {
                0 => "lights_highbeam",
                1 => "lights_mainbeam",
                _ => "",
            }
        } else if renown {
            match i {
                0 => "lights_lowbeam",
                1 => "lights_mainbeam",
                2 => "lights_highbeam",
                _ => "",
            }
        } else {
            ""
        };
        let sources: Vec<[f32; 3]> = if source_variable.is_empty() {
            Vec::new()
        } else {
            ty.model
                .meshes
                .iter()
                .flat_map(|m| {
                    m.light_enh
                        .iter()
                        .map(|l| (l.pos, l.variable.as_str()))
                        .chain(m.light_enh_2.iter().map(|l| (l.pos, l.variable.as_str())))
                })
                .filter_map(|(pos, variable)| (variable == source_variable).then_some(pos))
                .collect()
        };
        headlamp_lights(
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
                gain: bc.gain,
                range_mul: bc.range,
            },
            lights,
            coronas,
        );
    }
}

pub(super) const CODE_DIPPED: f32 = 200.0;
pub(super) const CODE_MAIN: f32 = 300.0;

const MAX_DROP: f32 = 0.03;

pub(super) fn headlamp_lights(
    h: &Headlamp,
    lights: &mut Vec<PointLight>,
    coronas: &mut Vec<Corona>,
) {
    let cfg = settings();
    let vals = h.vals;
    let raw_dir = Vec3::new(vals[3], vals[4], vals[5]).normalize_or_zero();
    if raw_dir == Vec3::ZERO {
        return;
    }
    let range = vals[9] * h.range_mul.max(0.05);
    let mut apex = Vec3::new(vals[0], vals[1], vals[2]);
    let dir_y = if raw_dir.y > 0.3 {
        1.0
    } else if raw_dir.y < -0.3 {
        -1.0
    } else {
        0.0
    };
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
    let local_dir = if dir_y != 0.0 {
        let a = aimed(raw_dir, aim_drop(apex.z));
        let hz = (a.x * a.x + a.y * a.y).sqrt().max(1e-4);
        Vec3::new(a.x / hz, a.y / hz, (a.z / hz).max(-MAX_DROP)).normalize_or_zero()
    } else {
        raw_dir
    };
    let color = [vals[6] / 255.0, vals[7] / 255.0, vals[8] / 255.0];
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
    let half = |deg: f32| (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos();

    let outer = vals[10].max(vals[11]).max(match h.kind {
        BeamKind::Dipped => 80.0,
        BeamKind::Main => 50.0,
    });

    let cone = if dir_y < 0.0 {
        [half(outer * 0.6), half(outer)]
    } else {
        [half(outer * 0.3), half(outer)]
    };
    let reach_v = spot_reach(range, 45.0);
    let radius = match h.kind {
        BeamKind::Dipped => spot_reach(range, 60.0).max(60.0),
        BeamKind::Main => range.clamp(MAIN_MIN_RANGE, 300.0),
    };
    let short = short_range_gain(range);
    let beam = match h.kind {
        BeamKind::Dipped => CODE_DIPPED,
        BeamKind::Main => CODE_MAIN,
    };

    let base_d = h.body.transform_vector3(local_dir).normalize_or_zero();
    if h.kind == BeamKind::Dipped {
        lights.push(PointLight {
            position: h.origin + (base + base_d * 0.35).as_dvec3(),
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
            .map(|side| (base + right * spread * side, apex.z))
            .collect()
    } else {
        h.sources
            .iter()
            .map(|source| (h.body.transform_point3(Vec3::from(*source)), source[2]))
            .collect()
    };
    for (spot_at, source_height) in sources {
        let source_dir = if dir_y != 0.0 {
            let a = aimed(raw_dir, aim_drop(source_height));
            let hz = (a.x * a.x + a.y * a.y).sqrt().max(1e-4);
            Vec3::new(a.x / hz, a.y / hz, (a.z / hz).max(-MAX_DROP)).normalize_or_zero()
        } else {
            raw_dir
        };
        let d = h.body.transform_vector3(source_dir).normalize_or_zero();
        let lamp = PointLight {
            position: h.origin + spot_at.as_dvec3(),
            color,
            direction: d,
            cone,
            ..Default::default()
        };
        if h.kind == BeamKind::Dipped && h.sources.is_empty() {
            let glare_at = h
                .lamps
                .iter()
                .map(|l| h.body.transform_point3(Vec3::from(*l)))
                .filter(|p| (*p - spot_at).length() < 0.6)
                .min_by(|a, b| (*a - spot_at).length().total_cmp(&(*b - spot_at).length()))
                .unwrap_or(spot_at + d * 0.05);
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
        lights.push(PointLight {
            radius: reach_v,
            intensity: cfg.vanilla * 0.5 * (0.3 + 0.7 * h.night) * short * h.level * h.gain,
            mode: LightMode::Vanilla,
            ..lamp
        });
        lights.push(PointLight {
            radius,
            intensity: cfg.headlight * 0.5 * short * h.level * h.gain,
            core: 1.0,
            beam,
            mode: LightMode::Enhanced,
            ..lamp
        });
    }
}
