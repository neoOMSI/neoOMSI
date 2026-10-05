use super::*;

fn read_list(r: &mut omsi_cfg::CfgReader, base: &Path) -> Vec<PathBuf> {
    let n = r.usize();
    (0..n)
        .map(|_| r.str().to_string())
        .filter(|s| !s.trim().is_empty())
        .map(|s| omsi_cfg::resolve_path(base, &s))
        .collect()
}

fn read_camera(r: &mut omsi_cfg::CfgReader, extra: bool) -> Camera {
    let pos = r.f32s::<3>();
    let dist = r.f32();
    let fov = r.f32();
    let yaw = r.f32();
    let pitch = r.f32();
    let extra = if extra { Some(r.f32()) } else { None };
    Camera {
        pos,
        dist,
        fov,
        yaw,
        pitch,
        extra,
    }
}

pub fn parse_attachment(r: &mut omsi_cfg::CfgReader) -> Attachment {
    let mut a = Attachment::default();
    loop {
        let save = r.pos();
        let l = r.str();
        let w = l.trim().to_ascii_lowercase();
        match w.as_str() {
            "attach_trans" => a.ops.push((w, r.f32s::<3>().to_vec())),
            "attach_rot_x" | "attach_rot_y" | "attach_rot_z" => a.ops.push((w, vec![r.f32()])),
            _ => {
                if omsi_cfg::keyword_of(l).is_some() || r.at_end() {
                    r.seek(save);
                    break;
                }
            }
        }
    }
    a
}

fn parse_axle(r: &mut omsi_cfg::CfgReader) -> Axle {
    let mut a = Axle::default();
    // Old files (the stock AI cars, the Manta) list the values without their keywords, one
    // per line: long, max width, min width, wheel diameter, spring, max force, damper,
    // driven, inertia. Read that way, the Golf's wheels are 0.503 m, not the default 1 m
    // that made every AI car's wheels turn at half their speed.
    let first = r
        .lines()
        .get(r.pos())
        .map(|l| l.trim().to_string())
        .unwrap_or_default();
    if omsi_cfg::keyword_of(&first).is_none() && first.parse::<f32>().is_ok() {
        let mut vals = Vec::new();
        while vals.len() < 9 && !r.at_end() {
            let save = r.pos();
            match r.word().parse::<f32>() {
                Ok(v) => vals.push(v),
                Err(_) => {
                    r.seek(save);
                    break;
                }
            }
        }
        let slots: [&mut f32; 7] = [
            &mut a.long,
            &mut a.max_width,
            &mut a.min_width,
            &mut a.wheel_diameter,
            &mut a.spring,
            &mut a.max_force,
            &mut a.damper,
        ];
        for (slot, v) in slots.into_iter().zip(vals.iter()) {
            *slot = *v;
        }
        if let Some(d) = vals.get(7) {
            a.driven = *d != 0.0;
        }
        if let Some(i) = vals.get(8) {
            a.inertia_inv = *i;
        }
        return a;
    }
    loop {
        let save = r.pos();
        let l = r.str();
        let w = l.trim().to_ascii_lowercase();
        match w.as_str() {
            "achse_long" => a.long = r.f32(),
            "achse_maxwidth" => a.max_width = r.f32(),
            "achse_minwidth" => a.min_width = r.f32(),
            "achse_raddurchmesser" => a.wheel_diameter = r.f32(),
            "achse_feder" => a.spring = r.f32(),
            "achse_maxforce" => a.max_force = r.f32(),
            "achse_daempfer" => a.damper = r.f32(),
            "achse_antrieb" => {
                let w = r.word();
                a.driven = w
                    .parse::<f32>()
                    .map(|x| x != 0.0)
                    .unwrap_or(w.eq_ignore_ascii_case("true"));
            }
            "achse_inertia_inv" => a.inertia_inv = r.f32(),
            _ => {
                if omsi_cfg::keyword_of(l).is_some() || r.at_end() {
                    r.seek(save);
                    break;
                }
                // comment line between the parameters: skip
            }
        }
    }
    a
}

impl Vehicle {
    pub fn parse(file: &CfgFile) -> Vehicle {
        let is_ovh = file
            .path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("ovh"))
            .unwrap_or(false);
        let mut v = Vehicle {
            path: file.path.clone(),
            kind: if is_ovh {
                VehicleKind::Other(0)
            } else {
                VehicleKind::Bus
            },
            mass: 1000.0,
            ..Default::default()
        };
        let base = file.dir().to_path_buf();
        let mut r = file.reader().disabled_blocks();
        while let Some(k) = r.next_keyword() {
            match k.as_str() {
                "type" => v.kind = VehicleKind::Other(r.i32()),
                "friendlyname" => {
                    v.has_friendly_name = true;
                    v.manufacturer = r.str().to_string();
                    v.type_name = r.str().to_string();
                    v.default_paint = r.str().to_string();
                }
                "friendlyname_inv" => {
                    v.friendly_name_inv = (0..3).map(|_| r.str().to_string()).collect()
                }
                "description" => v.description = r.until("[end]").join("\n"),
                "ai_veh_type" => v.ai_veh_type = r.i32(),
                "number" => v.number_file = Some(r.str().to_string()),
                // Omsi.exe (TRoadVehicle.LoadFromFile 0x7cddf5, 0x7cde6e) reads these lines
                // as they come, whatever they say: the automatic mode's prefix and postfix
                // (the stock "B-V " with its space), the list mode's file, prefix and postfix.
                // The lines end where the next block starts, so a file that leaves them out
                // (the Urumqi AI cars' `[registration_automatic]` straight before `[model]`)
                // reads them as empty instead of taking the keyword for one of them.
                "registration_automatic" => {
                    let pre = r.param_line().to_string();
                    let post = r.param_line().to_string();
                    v.registration_automatic = Some((pre.clone(), post.clone()));
                    v.registration_mode = 3;
                    v.registration_affix = (pre, post);
                }
                "registration_list" => {
                    let file = r.param_line().trim().to_string();
                    let pre = r.param_line().to_string();
                    let post = r.param_line().to_string();
                    v.registration_list = Some((file, pre.clone(), post.clone()));
                    v.registration_mode = 2;
                    v.registration_affix = (pre, post);
                }
                "registration_free" => {
                    v.registration_free = true;
                    v.registration_mode = 1;
                }
                "kmcounter_init" => {
                    let y = r.i32();
                    let km = r.f32();
                    v.km_counter_init = Some((y, km));
                }
                "sound" => v.sound = Some(r.str().to_string()),
                "sound_ai" => v.sound_ai = Some(r.str().to_string()),
                "model" => v.model = Some(r.str().to_string()),
                "paths" => v.paths = Some(r.str().to_string()),
                "passengercabin" => v.passenger_cabin = Some(r.str().to_string()),
                "varnamelist" => v.scripts.varlists.extend(read_list(&mut r, &base)),
                "stringvarnamelist" => v.scripts.stringvarlists.extend(read_list(&mut r, &base)),
                "script" => v.scripts.scripts.extend(read_list(&mut r, &base)),
                "constfile" => v.scripts.constfiles.extend(read_list(&mut r, &base)),
                "scriptshare" => v.script_share = true,
                "add_camera_driver" => v.cameras_driver.push(read_camera(&mut r, false)),
                "add_camera_pax" => v.cameras_pax.push(read_camera(&mut r, false)),
                "add_camera_reflexion" => v.cameras_reflexion.push(read_camera(&mut r, false)),
                "add_camera_reflexion_2" => v.cameras_reflexion.push(read_camera(&mut r, true)),
                "view_schedule" => v.view_schedule = Some(v.cameras_driver.len().saturating_sub(1)),
                "view_ticketselling" => {
                    v.view_ticketselling = Some(v.cameras_driver.len().saturating_sub(1))
                }
                "set_camera_std" => v.camera_std = r.usize(),
                "set_camera_outside_center" => v.camera_outside_center = r.f32s::<3>(),
                "mass" => v.mass = r.f32(),
                "momentofintertia" => v.moment_of_inertia = r.f32s::<3>(),
                "boundingbox" => v.bounding_box = Some(r.f32s::<6>()),
                "cog" => v.cog = Some(r.f32s::<3>()),
                "schwerpunkt" => v.cog_height = r.f32(),
                "rollwiderstand" => v.rolling_resistance = r.f32(),
                "rot_pnt_long" => v.rot_pnt_long = r.f32(),
                "inv_min_turnradius" => v.inv_min_turn_radius = r.f32(),
                "ai_deltaheight" => v.ai_delta_height = r.f32(),
                "newachse" => v.axles.push(parse_axle(&mut r)),
                "new_attachment" => v.attachments.push(parse_attachment(&mut r)),
                "coupling_front" => v.coupling_front = Some(Coupling { pos: r.f32s::<3>() }),
                "coupling_back" => v.coupling_back = Some(Coupling { pos: r.f32s::<3>() }),
                "couple_front" | "couple_back" | "couple" => {
                    let f = r.str().to_string();
                    let save = r.pos();
                    let b = r.word();
                    let flag = if b.eq_ignore_ascii_case("true") {
                        true
                    } else if b.eq_ignore_ascii_case("false") {
                        false
                    } else {
                        r.seek(save);
                        false
                    };
                    if k == "couple_back" {
                        v.couple_back = Some((f, flag));
                    } else {
                        v.couple_front = Some((f, flag));
                    }
                }
                "couple_front_open_for_sound" => v.couple_front_open_for_sound = true,
                "coupling_front_character" => v.coupling_front_character = Some(r.f32s::<4>()),
                "control_cable_front" => v.control_cable_front.push(ControlCable {
                    lines: (0..5).map(|_| r.str().to_string()).collect(),
                }),
                "control_cable_back" => v.control_cable_back.push(ControlCable {
                    lines: (0..5).map(|_| r.str().to_string()).collect(),
                }),
                "rowdy_factor" => {
                    let a = r.f32();
                    let b = r.f32();
                    v.rowdy_factor = Some((a, b));
                }
                "boogies" => v.boogies = Some(r.f32()),
                "sinus" => v.sinus = Some(r.f32s::<4>()),
                "rail_body_osc" => v.rail_body_osc = Some(r.f32s::<7>()),
                "contact_shoe" => v.contact_shoes.push(r.f32s::<6>()),
                "ai_brakeperformance" => v.ai_brake_performance = Some(r.f32s::<5>()),
                "fixed" => v.fixed = true,
                _ => v.unknown_keywords.push((k, r.block_line())),
            }
        }
        v
    }
}
