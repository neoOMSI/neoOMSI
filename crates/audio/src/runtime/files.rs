//! File-trigger announcements and HTML page playback; same legacy level/order rules.
use super::soundset::SoundSet;
use super::sound::{warn_missing_once, volume_side, SoundState};
use super::{outside, placement, level::SOUND_MAXCOUNT};
use crate::engine::Playback;
use crate::voice::{Level, MixParams, VoiceParams};
use std::path::Path;
use glam::Mat4;

impl SoundSet {
    /// Play the sound of a `(T.F.trigger)` event: the entry listening to `trigger`
    /// plays `file` (relative to the sound folder) with its own volume and position.
    pub fn play_file_trigger(
        &mut self,
        engine: &dyn Playback,
        trigger: &str,
        file: &str,
        var: &dyn Fn(&str) -> Option<f32>,
        object_to_world: &Mat4,
    ) {
        if !engine.enabled() || file.trim().is_empty() {
            return;
        }
        let path = ::mods::resolve_path(&self.dir, file);
        let Some(clip) = engine.load_clip(&path) else {
            warn_missing_once(trigger, &path);
            return;
        };
        // `(T.F.)` is the generic dynamic-file trigger, not a synonym for an announcement:
        // the C2 gearbox and other scripted sounds fire it too. Only a file resolved under
        // an `Announcements` folder gets the PA bus; everything else stays on the set's
        // fallback `file_bus` (the vehicle), so it is not fed the PA hall/leakage path.
        let bus = if is_announcement_path(&path) {
            crate::Bus::Announcement
        } else {
            self.file_bus
        };
        let ai = self.ai;
        let (eased, muffled) = self.advance_blend();
        let inside = self.inside_factor(eased, object_to_world, engine.listener_position());
        let (exterior, master) = (self.exterior, self.master);

        let now = self.clock.now();
        let mut used = self
            .sounds
            .iter()
            .filter(|s| s.voice.is_some_and(|id| engine.is_playing(id)))
            .count();
        for s in self.sounds.iter_mut() {
            if !s
                .def
                .triggers
                .iter()
                .any(|d| d.eq_ignore_ascii_case(trigger))
            {
                continue;
            }
            // the runtime admission budget: a `(T.F.)` sound past `[sound_maxcount]` and with
            // no voice of its own does not start
            let running = s.voice.is_some_and(|id| engine.is_playing(id));
            if !running && used >= SOUND_MAXCOUNT {
                continue;
            }
            s.active_since = Some(now);
            let Some((record, script, through)) = volume_side(&s.def, var, ai, 0.0, 1.0) else {
                continue;
            };
            if !running {
                used += 1;
            }
            let (position, reach, pan) =
                placement::place(s.def.pos, s.def.range, exterior, object_to_world);
            let (body_gain, lowpass_hz) = if bus == crate::Bus::Announcement {
                outside::announcement_transfer(&s.def, exterior, inside, muffled, var("Snd_OutsideVol"))
            } else { outside::entry_transfer(&s.def, exterior, inside, muffled) };
            let (spatial_blend, pan_width) = placement::cabin_spatial(s.def.pos, exterior, inside);
            s.peak = record * script * through;
            s.playback_bus = Some(bus);
            let params = MixParams {
                cabin_reverb: outside::announcement_reverb(&s.def, bus, exterior,
                    inside, self.announcement_reverb),
                spatial_blend,
                bus,
                level: Level::Omsi {
                    record,
                    script,
                    transmission: through * body_gain,
                    master,
                },
                pitch: 1.0,
                looping: false,
                position,
                doppler: !self.listener_vehicle,
                range: reach,
                lowpass_hz,
                important: s.def.important,
                pan: pan * pan_width,
            };
            if let Some(id) = s.voice.take() {
                engine.stop(id);
            }
            s.clip = Some(clip.clone());
            s.voice = Some(engine.play_mix(clip.clone(), params));
            s.state = SoundState::Running;
        }
    }

    /// Play `path` once, non-spatial, at `volume` (0..1): a sound a page asks for
    /// (`omsi.playSound`) that has no entry of its own in the `sound.cfg`.
    pub fn play_file_direct(&mut self, engine: &dyn Playback, path: &Path, volume: f32) {
        if !engine.enabled() {
            return;
        }
        let Some(clip) = engine.load_clip(path) else {
            warn_missing_once("playSound", path);
            return;
        };
        let params = VoiceParams {
            gain: volume.clamp(0.0, 1.0) * self.master,
            pitch: 1.0,
            looping: false,
            position: None,
            doppler: false,
            range: 5.0,
            lowpass_hz: 0.0,
            important: false,
            pan: 1.0,
        };
        let mut mix = MixParams::from(params);
        mix.bus = crate::voice::bus::Bus::Interface;
        engine.play_mix(clip, mix);
    }

}

/// Whether a resolved `(T.F.)` file lies in an `Announcements` folder (case-insensitive):
/// those carry the PA/announcement bus, all other dynamic `[sound] N` files do not.
fn is_announcement_path(path: &Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("Announcements")
    })
}
