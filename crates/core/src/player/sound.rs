use super::*;

impl Player {
    pub(crate) fn tick_sounds(
        &mut self,
        audio: Option<&::audio::AudioEngine>,
        inside: bool,
        listener_follows_bus: bool,
        driven: bool,
    ) {
        let fired: Vec<::simulation::host::FiredSound> =
            std::mem::take(&mut self.vehicle.host.fired_sounds);
        let fired_vars: Vec<(String, Vec<f32>)> =
            std::mem::take(&mut self.vehicle.host.fired_trigger_vars);
        for s in &fired {
            if let Some(f) = s.file() {
                log::info!("announcement: {} -> {f}", s.name());
            }
        }
        let html_sounds: Vec<(String, f32)> = std::mem::take(&mut self.vehicle.host.html_sounds);
        if let (Some(a), Some(ss)) = (audio, self.sounds.as_mut()) {
            let xf = self.vehicle.world_transform();
            let v = &self.vehicle;
            ss.set_inside(inside && driven);
            let cabin = ss.prepare_listener_cabin(a, &xf,
                                                  &|i| v.trailers.get(i).map(|t| t.world_transform()));
            let acoustic_inside = inside || cabin > 0.5;
            ss.set_muffled(acoustic_inside);
            ss.set_listener_vehicle(listener_follows_bus && driven);
            if driven {
                ::audio::soundset::set_outside_open(if acoustic_inside {
                    v.var("Snd_OutsideVol")
                } else {
                    None
                });
            }
            let events = crate::sound_events::events_from(
                ::audio::EventSource::Player,
                &fired,
                &fired_vars,
            );
            ss.update_events(a, &|n| v.var(n), &xf, &events, &|n| v.var_slot(n));
            ss.update_parts_events(
                a,
                &|n| v.var(n),
                &|i| v.trailers.get(i).map(|t| t.world_transform()),
                &events,
                &|n| v.var_slot(n),
            );
            for (f, vol) in &html_sounds {
                let path = ::mods::resolve_path(v.ty.def.dir(), f);
                ss.play_file_direct(a, &path, *vol);
            }
            if let Some(every) = weather::debug_sound_every() {
                static LAST: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(u32::MAX);
                let bucket = (v.host.clock.time / every as f64) as u32;
                if LAST.swap(bucket, std::sync::atomic::Ordering::Relaxed) != bucket {
                    log::info!(
                        "sound: the player's bus ({} entries, {})",
                        ss.len(),
                        if inside { "inside" } else { "outside" }
                    );
                    for line in ss.report(a, &|n| v.var(n)) {
                        log::info!("  {line}");
                    }
                    for (i, part) in &ss.parts {
                        log::info!("sound: coupled part {i} ({} entries)", part.len());
                        for line in part.report(a, &|n| v.var(n)) {
                            log::info!("  {line}");
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn load_sounds(&mut self, audio: &::audio::AudioEngine) {
        let def = &self.vehicle.ty.def;
        if let Some(rel) = &def.sound {
            let path = ::mods::resolve_path(def.dir(), rel);
            match ::legacy_vehicle::SoundCfg::load(&path) {
                Ok(cfg) => {
                    let number = self.vehicle.number();
                    let all = cfg.sounds.len();
                    let cfg = cfg.chosen_for(&number);
                    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                    let mut ss = ::audio::SoundSet::new(audio, &cfg, &dir);
                    ss.set_hull(def.bounding_box);
                    log::info!(
                        "sound config {}: {} sounds ({} of {all} for fleet number '{number}')",
                        path.display(),
                        cfg.sounds.len(),
                        cfg.sounds.len()
                    );
                    let mut connected_cabin = true;
                    for (i, t) in self.vehicle.trailers.iter().enumerate() {
                        connected_cabin &= t.ty.def.couple_front_open_for_sound && !t.reversed;
                        let Some(rel) = &t.ty.def.sound else { continue };
                        let path = ::mods::resolve_path(t.ty.def.dir(), rel);
                        match ::legacy_vehicle::SoundCfg::load(&path) {
                            Ok(cfg) => {
                                let dir =
                                    path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                                log::info!(
                                    "sound config of coupled part {i} {}: {} sounds",
                                    path.display(),
                                    cfg.sounds.len()
                                );
                                let mut part = ::audio::SoundSet::new(
                                    audio,
                                    &cfg.chosen_for(&number),
                                    &dir,
                                );
                                part.set_hull(t.ty.def.bounding_box);
                                part.set_cabin_connected(connected_cabin);
                                ss.add_part(i, part);
                            }
                            Err(error) => {
                                log::warn!("{error}");
                                ::mods::diagnostics::missing(t.ty.def.dir(), &path, ::mods::diagnostics::Kind::Sound);
                            }
                        }
                    }
                    self.vehicle.host.snapshot_triggers = ss.curve_triggers().into_iter().collect();
                    self.sounds = Some(ss);
                }
                Err(error) => {
                    log::warn!("{error}");
                    ::mods::diagnostics::missing(def.dir(), &path, ::mods::diagnostics::Kind::Sound);
                }
            }
        }
    }
}
