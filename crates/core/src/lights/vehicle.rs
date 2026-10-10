use super::*;

pub fn vehicle_lights(
    v: &VehicleInstance,
    coronas: &mut Vec<Corona>,
    lights: &mut Vec<PointLight>,
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
    let cfg = settings();
    let night = night.max(weather_darkness() * cfg.weather_night);
    headlamps(v, night, lights, coronas);
    spotlights_2(
        &ty.model,
        v.body_rotation(),
        v.position,
        key_of(v),
        &value_of,
        night,
        lights,
    );
    for t in &v.trailers {
        spotlights_2(
            &t.ty.model,
            t.body_rotation(),
            t.position,
            key_of(t),
            &value_of,
            night,
            lights,
        );
    }
    if spill && cfg.spill.on && night > 0.05 {
        interior_spill(v, &value_of, night, lights);
    }
}
