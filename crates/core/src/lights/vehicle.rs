use super::*;

#[derive(Clone, Hash, PartialEq, Eq)]
struct CookieKey {
    model_dir: std::path::PathBuf,
    name: String,
}

#[derive(Clone)]
enum CachedCookie {
    Missing,
    Invalid,
    Loaded {
        path: std::path::PathBuf,
        image: std::sync::Weak<::texture::Image>,
    },
}

type CookieImageCache = std::collections::HashMap<CookieKey, CachedCookie>;

static COOKIE_CACHE: std::sync::OnceLock<std::sync::Mutex<CookieImageCache>> =
    std::sync::OnceLock::new();
static COOKIE_CAPACITY_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn cookie_cache() -> &'static std::sync::Mutex<CookieImageCache> {
    COOKIE_CACHE.get_or_init(Default::default)
}

pub(crate) fn invalidate_cookie_cache(model_dir: &std::path::Path) {
    if let Some(cache) = COOKIE_CACHE.get() {
        cache
            .lock()
            .unwrap()
            .retain(|key, _| key.model_dir.as_path() != model_dir);
    }
}

fn cookie_image(
    ty: &::simulation::VehicleType,
    name: &str,
) -> Option<(std::path::PathBuf, std::sync::Arc<::texture::Image>)> {
    let name = name.trim();
    if name.is_empty() || ::legacy_config::env::var_os("OMSI_NO_COOKIES").is_some() {
        return None;
    }
    let key = CookieKey {
        model_dir: ty.model_dir.clone(),
        name: name.to_string(),
    };
    let texture_dir = ::legacy_config::resolve_path(&ty.model_dir, "Texture");
    let mut dirs = vec![texture_dir, ty.model_dir.clone(), ty.def.dir().to_path_buf()];
    dirs.extend(
        ::legacy_config::content_roots()
            .into_iter()
            .map(|root| root.join("Texture")),
    );
    cookie_image_at(key, &dirs)
}

fn cookie_image_at(
    key: CookieKey,
    dirs: &[std::path::PathBuf],
) -> Option<(std::path::PathBuf, std::sync::Arc<::texture::Image>)> {
    let name = key.name.as_str();
    let cached = cookie_cache().lock().unwrap().get(&key).cloned();
    let path = match cached {
        Some(CachedCookie::Missing | CachedCookie::Invalid) => return None,
        Some(CachedCookie::Loaded { path, image }) => {
            if let Some(image) = image.upgrade() {
                return Some((path, image));
            }
            path
        }
        None => {
            let dir_refs: Vec<&std::path::Path> = dirs.iter().map(|dir| dir.as_path()).collect();
            let Some(path) = ::texture::find_texture(name, &dir_refs) else {
                let mut cache = cookie_cache().lock().unwrap();
                let first = !cache.contains_key(&key);
                cache.entry(key.clone()).or_insert(CachedCookie::Missing);
                drop(cache);
                if first {
                    log::warn!("beam cookie {name:?} is missing under {}", key.model_dir.display());
                }
                return None;
            };
            path
        }
    };

    match ::texture::decode_file(&path) {
        Ok(image) => {
            let image = std::sync::Arc::new(image);
            let mut cache = cookie_cache().lock().unwrap();
            match cache.get_mut(&key) {
                Some(CachedCookie::Missing | CachedCookie::Invalid) => None,
                Some(CachedCookie::Loaded {
                    path: cached_path,
                    image: cached_image,
                }) => {
                    if let Some(cached_image) = cached_image.upgrade() {
                        Some((cached_path.clone(), cached_image))
                    } else {
                        *cached_path = path.clone();
                        *cached_image = std::sync::Arc::downgrade(&image);
                        Some((path, image))
                    }
                }
                None => {
                    cache.insert(
                        key,
                        CachedCookie::Loaded {
                            path: path.clone(),
                            image: std::sync::Arc::downgrade(&image),
                        },
                    );
                    Some((path, image))
                }
            }
        }
        Err(error) => {
            let mut cache = cookie_cache().lock().unwrap();
            let mut warn = false;
            let result = match cache.get(&key).cloned() {
                Some(CachedCookie::Loaded {
                    path: cached_path,
                    image,
                }) => match image.upgrade() {
                    Some(image) => Some((cached_path, image)),
                    None => {
                        cache.insert(key, CachedCookie::Invalid);
                        warn = true;
                        None
                    }
                },
                Some(CachedCookie::Missing | CachedCookie::Invalid) => None,
                None => {
                    cache.insert(key, CachedCookie::Invalid);
                    warn = true;
                    None
                }
            };
            drop(cache);
            if warn {
                log::warn!("beam cookie {} could not be loaded: {error}", path.display());
            }
            result
        }
    }
}

#[derive(Default)]
pub(crate) struct CookieSlots {
    by_path: std::collections::HashMap<std::path::PathBuf, u8>,
    textures: Vec<::render::CookieTexture>,
}

impl CookieSlots {
    pub(crate) fn into_textures(self) -> Vec<::render::CookieTexture> {
        self.textures
    }

    fn assign(
        &mut self,
        path: std::path::PathBuf,
        image: std::sync::Arc<::texture::Image>,
    ) -> Option<u8> {
        if let Some(slot) = self.by_path.get(&path) {
            return Some(*slot);
        }
        if self.textures.len() == ::render::COOKIE_SLOTS {
            if !COOKIE_CAPACITY_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::warn!("beam cookies support {} simultaneous images", ::render::COOKIE_SLOTS);
            }
            return None;
        }
        let slot = self.textures.len() as u8 + 1;
        self.by_path.insert(path, slot);
        self.textures.push(::render::CookieTexture { slot, image });
        Some(slot)
    }

    fn slot(&mut self, ty: &::simulation::VehicleType, name: &str) -> Option<u8> {
        let (path, image) = cookie_image(ty, name)?;
        self.assign(path, image)
    }
}

pub fn vehicle_lights(
    v: &VehicleInstance,
    coronas: &mut Vec<Corona>,
    lights: &mut Vec<PointLight>,
    cookies: &mut CookieSlots,
    night: f32,
    spill: bool,
) {
    let ty = &v.ty;
    let value_of = |name: &str| -> f32 {
        let t = name.trim();
        if let Ok(x) = t.parse::<f32>() {
            return x;
        }
        v.var(t).unwrap_or(0.0)
    };
    let mesh_xf = |def_index: usize| -> glam::Mat4 {
        match ty.meshes.iter().position(|m| m.def_index == def_index) {
            Some(i) => v.mesh_local_transform(i),
            None => v.body_rotation(),
        }
    };
    let own_rot = v.body_rotation();
    for (mut c, owner) in crate::scene::model_lights_owned(
        &ty.model,
        &mesh_xf,
        v.position,
        &value_of,
        &v.light_fade,
    ) {
        let ec = exterior_cfg(owner);
        if ec.off {
            continue;
        }
        c.brightness *= ec.gain;
        c.size *= ec.size.max(0.0);
        c.spread = ec.spread.max(0.05);
        c.color = [
            c.color[0] * ec.color[0],
            c.color[1] * ec.color[1],
            c.color[2] * ec.color[2],
        ];
        c.position += own_rot.transform_vector3(Vec3::from(ec.shift)).as_dvec3();
        coronas.push(c);
    }
    for t in &v.trailers {
        let part_mesh_xf = |def_index: usize| -> glam::Mat4 {
            match t.ty.meshes.iter().position(|m| m.def_index == def_index) {
                Some(i) => t.mesh_local_transform(i),
                None => t.body_rotation(),
            }
        };
        coronas.extend(crate::scene::model_lights_faded(
            &t.ty.model,
            &part_mesh_xf,
            t.position,
            &value_of,
            &t.light_fade,
        ));
    }
    let body = v.body_rotation();
    let forced = ::legacy_config::env::var("OMSI_SPOT_SELECT")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok());
    let ai_on = v.ai_lights;
    let cfg = settings();
    let bad = weather_darkness();
    let night = night.max(bad * cfg.weather_night);
    let selected = forced
        .or_else(|| v.var("Spot_Select"))
        .filter(|s| *s >= 0.0 || !ai_on);
    if let Some(sel) = selected.or(ai_on.then_some(0.0)) {
        if sel >= 0.0 {
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
            let spot = ty
                .model
                .spotlights
                .get(sel as usize)
                .or_else(|| ai_on.then(|| ty.model.spotlights.first()).flatten())
                .map(|sp| sp.values)
                .or_else(|| ai_on.then(|| ai_spotlight(&lamps)).flatten());
            if let Some(vals) = spot {
                let d = body
                    .transform_vector3(Vec3::new(vals[3], vals[4], vals[5]))
                    .normalize_or_zero();
                let high_beam =
                    cfg.force_high_beam || v.var("lights_fern").is_some_and(|x| x > 0.5);
                let bc = if high_beam { cfg.high } else { cfg.low };
                let color = [
                    vals[6] / 255.0 * cfg.lamp_color[0] * bc.color[0],
                    vals[7] / 255.0 * cfg.lamp_color[1] * bc.color[1],
                    vals[8] / 255.0 * cfg.lamp_color[2] * bc.color[2],
                ];
                let mut apex = Vec3::new(vals[0], vals[1], vals[2]);
                let dl = Vec3::new(vals[3], vals[4], vals[5]).normalize_or_zero();
                let nose = lamps.iter().map(|l| l[1]).reduce(f32::max);
                let tail = lamps.iter().map(|l| l[1]).reduce(f32::min);
                let bb = ty
                    .def
                    .bounding_box
                    .map(|bb| (bb[4] + bb[1] * 0.5, bb[4] - bb[1] * 0.5));
                let dir = if dl.y > 0.3 {
                    1.0
                } else if dl.y < -0.3 {
                    -1.0
                } else {
                    0.0
                };
                if dir != 0.0 {
                    let (lamp, edge) = if dir > 0.0 {
                        (nose, bb.map(|b| b.0))
                    } else {
                        (tail, bb.map(|b| b.1))
                    };
                    if let Some(face) = spot_face(lamp, edge, apex.y, dir) {
                        apex.y = face + dir * 0.05;
                    }
                }
                let half_width = ty
                    .def
                    .bounding_box
                    .map_or(1.25, |bb| (bb[0] * 0.5).min(1.25));
                let on_face: Vec<f32> = lamps
                    .iter()
                    .filter(|l| (dl.y > 0.3 || dl.y < -0.3) && (l[1] - apex.y).abs() < 0.35)
                    .map(|l| (l[0] - apex.x).abs())
                    .collect();
                let spread =
                    (on_face.iter().sum::<f32>() / on_face.len().max(1) as f32).min(half_width);
                let right = body.transform_vector3(Vec3::X).normalize_or_zero();
                let apex = body.transform_point3(apex);
                let (inner, outer) = (
                    vals.get(10).copied().unwrap_or(30.0) + cfg.lamp_inner_add + bc.inner_add,
                    vals.get(11).copied().unwrap_or(70.0) + cfg.lamp_outer_add + bc.outer_add,
                );
                let half = |deg: f32| (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos();
                let cone = [half(inner.min(outer)), half(outer)];
                let sides: &[f32] = if spread > 0.1 { &[-1.0, 1.0] } else { &[0.0] };
                for side in sides {
                    if !bc.on {
                        continue;
                    }
                    let at = v.position
                        + (apex + right * spread * cfg.lamp_spread * side).as_dvec3()
                        + lamp_shift(d, &cfg)
                        + shift(d, bc.forward, bc.side, bc.height);
                    let radius = headlight_radius(vals[9])
                        * cfg.lamp_range.max(0.05)
                        * bc.range.max(0.05)
                        * if high_beam { cfg.high_beam_range } else { 1.0 };
                    let cone = if high_beam {
                        let k = cfg.high_beam_spread.max(0.1);
                        [1.0 - (1.0 - cone[0]) * k, 1.0 - (1.0 - cone[1]) * k]
                    } else {
                        cone
                    };
                    let lamp = PointLight {
                        position: at,
                        radius,
                        color,
                        direction: lamp_aim(d, &cfg, &bc),
                        cone,
                        ..Default::default()
                    };
                    lights.push(PointLight {
                        intensity: cfg.vanilla / sides.len() as f32 * (0.3 + 0.7 * night),
                        mode: LightMode::Vanilla,
                        ..lamp
                    });
                    lights.push(PointLight {
                        intensity: cfg.headlight / sides.len() as f32
                            * (1.0 + bad * cfg.weather_boost)
                            * if high_beam { cfg.high_beam } else { 1.0 }
                            * bc.gain,
                        core: headlight_core(radius) * cfg.lamp_core.max(0.01) * bc.core.max(0.01),
                        beam: if high_beam { -1.0 } else { cfg.low_beam_gain },
                        mode: LightMode::Enhanced,
                        ..lamp
                    });
                }
            }
        }
    }
    spotlights_2(&ty.model, body, v.position, &value_of, night, lights);
    cookie_spotlights(
        ty,
        body,
        v.position,
        &value_of,
        &v.cookie_fade,
        lights,
        cookies,
    );
    for t in &v.trailers {
        spotlights_2(&t.ty.model, t.body_rotation(), t.position, &value_of, night, lights);
        cookie_spotlights(
            &t.ty,
            t.body_rotation(),
            t.position,
            &value_of,
            &t.cookie_fade,
            lights,
            cookies,
        );
    }
    if spill && cfg.spill.on && night > 0.05 {
        let mut sections: Vec<(&::model::Model, Option<[f32; 6]>, glam::Mat4, DVec3)> =
            vec![(&ty.model, body_box(ty), body, v.position)];
        for t in &v.trailers {
            sections.push((&t.ty.model, body_box(&t.ty), t.body_rotation(), t.position));
        }
        let sp = cfg.spill;
        let tilt = (INTERIOR_SPILL_TILT + sp.tilt_add).to_radians();
        let cone = [
            ((INTERIOR_SPILL_INNER + sp.inner_add) * sp.spread.max(0.05))
                .clamp(1.0, 179.0)
                .to_radians()
                .cos(),
            ((INTERIOR_SPILL_OUTER + sp.outer_add) * sp.spread.max(0.05))
                .clamp(1.0, 179.0)
                .to_radians()
                .cos(),
        ];
        let spill_r = spill_radius(&sp);
        for (model, _bb, xf, origin) in sections {
            let lit: Vec<usize> = (0..model.interior_lights.len())
                .filter(|&li| {
                    let il = &model.interior_lights[li];
                    value_of(&il.variable) >= 0.5 && !interior_cfg(li).off
                })
                .collect();
            if lit.is_empty() {
                continue;
            }
            let strength = night.clamp(0.0, 1.0) * sp.gain;
            for li in lit {
                let il = &model.interior_lights[li];
                let ic = interior_cfg(li);
                let at = Vec3::from(il.pos) + Vec3::from(ic.shift);
                let col = Vec3::from(il.color) / 255.0 * Vec3::from(ic.color);
                let color = (col * (1.0 - INTERIOR_SPILL_WHITE)
                    + Vec3::splat(col.max_element()) * INTERIOR_SPILL_WHITE)
                    .to_array();
                let sides = [Vec3::X, -Vec3::X];
                let gain = INTERIOR_SPILL_LAMP * ic.gain * strength / sides.len() as f32;
                for out in sides {
                    let dir = (out * tilt.cos() - Vec3::Z * tilt.sin()).normalize();
                    lights.push(PointLight {
                        position: origin + xf.transform_point3(at).as_dvec3(),
                        radius: spill_r,
                        color,
                        intensity: gain,
                        direction: xf.transform_vector3(dir).normalize_or_zero(),
                        cone,
                        core: INTERIOR_SPILL_CORE * sp.core.max(0.01),
                        mode: LightMode::Enhanced,
                        shadow_first: true,
                        ..Default::default()
                    });
                }
            }
        }
    }
}

fn cookie_spotlights(
    ty: &::simulation::VehicleType,
    rot: glam::Mat4,
    origin: DVec3,
    value_of: &impl Fn(&str) -> f32,
    fades: &[f32],
    lights: &mut Vec<PointLight>,
    cookies: &mut CookieSlots,
) {
    let up = rot.transform_vector3(Vec3::Z).normalize_or_zero();
    let right = rot.transform_vector3(Vec3::X).normalize_or_zero();
    for (i, sp) in ty.model.spotlights_cookie.iter().enumerate() {
        let level = fades
            .get(i)
            .copied()
            .unwrap_or_else(|| value_of(&sp.variable).clamp(0.0, 1.0))
            .clamp(0.0, 1.0);
        let finite_params = sp
            .position
            .iter()
            .chain(sp.direction.iter())
            .all(|v| v.is_finite());
        if !level.is_finite()
            || level <= 0.001
            || !sp.range.is_finite()
            || sp.range <= 0.0
            || Vec3::from(sp.direction).length_squared() <= 1e-6
            || !finite_params
        {
            continue;
        }
        let slot = cookies.slot(ty, &sp.texture).unwrap_or(0);
        let h = finite_value(value_of(&sp.h_offset)).to_radians();
        let v = finite_value(value_of(&sp.v_offset)).to_radians();
        let (sin_h, cos_h) = (-h).sin_cos();
        let (sin_v, cos_v) = v.sin_cos();
        let mirrors: &[f32] = if sp.mirrored { &[-1.0, 1.0] } else { &[0.0] };
        for mirror in mirrors {
            let local = Vec3::new(
                if *mirror == 0.0 { sp.position[0] } else { sp.position[0] * *mirror },
                sp.position[1],
                sp.position[2],
            );
            let local_dir = Vec3::new(
                if *mirror == 0.0 { sp.direction[0] } else { sp.direction[0] * *mirror },
                sp.direction[1],
                sp.direction[2],
            )
            .normalize_or_zero();
            let yawed = Vec3::new(
                local_dir.x * cos_h - local_dir.y * sin_h,
                local_dir.x * sin_h + local_dir.y * cos_h,
                local_dir.z,
            );
            let dir0 = rot.transform_vector3(yawed).normalize_or_zero();
            let axis = right;
            let dir = (dir0 * cos_v
                + axis.cross(dir0) * sin_v
                + axis * axis.dot(dir0) * (1.0 - cos_v))
            .normalize_or_zero();
            if dir.length_squared() <= 1e-6 {
                continue;
            }
            let at = origin + rot.transform_point3(local).as_dvec3();
            let (color, cone, cookie) = if slot == 0 {
                (
                    [1.0, 1.0, 233.0 / 255.0],
                    [15.0f32.to_radians().cos(), 50.0f32.to_radians().cos()],
                    0,
                )
            } else {
                ([1.0; 3], [1.0, 0.0], slot)
            };
            lights.push(PointLight {
                position: at,
                radius: sp.range,
                color,
                intensity: level,
                direction: dir,
                cone,
                core: (sp.range * 0.125).max(0.01),
                mode: LightMode::Enhanced,
                cookie,
                cookie_up: projected_cookie_up(dir, up),
                ..Default::default()
            });
        }
    }
}

fn finite_value(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn projected_cookie_up(forward: Vec3, up: Vec3) -> Vec3 {
    let forward = forward.normalize_or_zero();
    let project = |axis: Vec3| (axis - forward * axis.dot(forward)).normalize_or_zero();
    let up = project(up);
    if up.length_squared() > 1e-6 {
        return up;
    }
    project(if forward.z.abs() > 0.99 {
        Vec3::Y
    } else {
        Vec3::Z
    })
}

#[cfg(test)]
mod cookie_tests {
    use super::*;

    #[test]
    fn cookie_projection_up_is_stable_for_vertical_beams() {
        for forward in [Vec3::Z, -Vec3::Z, Vec3::new(1e-7, 0.0, 1.0)] {
            let up = projected_cookie_up(forward, Vec3::Z);
            assert!(up.is_finite());
            assert!((up.length() - 1.0).abs() < 1e-5);
            assert!(up.dot(forward.normalize()).abs() < 1e-5);
        }
    }

    #[test]
    fn cookie_slots_are_reused_for_each_light_collection() {
        let image = || std::sync::Arc::new(::texture::Image::solid([255; 4]));
        let mut current = CookieSlots::default();
        for i in 0..::render::COOKIE_SLOTS {
            assert_eq!(
                current.assign(format!("cookie-{i}").into(), image()),
                Some(i as u8 + 1)
            );
        }
        assert_eq!(current.assign("cookie-9".into(), image()), None);
        drop(current);

        let mut next = CookieSlots::default();
        assert_eq!(next.assign("another-cookie".into(), image()), Some(1));
    }

    #[test]
    fn invalid_cookie_images_fall_back_and_are_not_decoded_each_frame() {
        let root = std::env::temp_dir().join(format!(
            "neoomsi-cookie-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("broken.png"), b"not an image").unwrap();
        let key = CookieKey {
            model_dir: root.clone(),
            name: "broken.png".into(),
        };
        assert!(cookie_image_at(key.clone(), std::slice::from_ref(&root)).is_none());
        assert!(cookie_image_at(key.clone(), std::slice::from_ref(&root)).is_none());
        assert!(matches!(
            cookie_cache().lock().unwrap().get(&key),
            Some(CachedCookie::Invalid)
        ));
        cookie_cache().lock().unwrap().remove(&key);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reloading_a_vehicle_invalidates_its_cookie_failures() {
        let model_dir = std::path::PathBuf::from("reload-cookie-cache-test");
        let other_dir = std::path::PathBuf::from("other-cookie-cache-test");
        let key = |model_dir: std::path::PathBuf| CookieKey {
            model_dir,
            name: "missing.png".into(),
        };
        cookie_cache().lock().unwrap().insert(
            key(model_dir.clone()),
            CachedCookie::Missing,
        );
        cookie_cache().lock().unwrap().insert(
            key(other_dir.clone()),
            CachedCookie::Missing,
        );

        invalidate_cookie_cache(&model_dir);

        let cache = cookie_cache().lock().unwrap();
        assert!(!cache.contains_key(&key(model_dir)));
        assert!(cache.contains_key(&key(other_dir)));
    }
}
