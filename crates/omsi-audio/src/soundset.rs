//! Runtime for a sound configuration attached to an object with script variables.

use crate::mixer::{AudioEngine, Clip, VoiceId, VoiceParams};
use glam::{Mat4, Vec3};
use omsi_vehicle::{SoundCfg, SoundEntry};
use std::path::Path;
use std::sync::Arc;

struct RuntimeSound {
    def: SoundEntry,
    clip: Option<Arc<Clip>>,
    voice: Option<VoiceId>,
    /// The conditions held last frame (a `[noloop]` entry without a trigger plays once
    /// when they start to hold).
    held: bool,
    /// Since when the conditions hold (triggered entries: since the trigger last fired) -
    /// what a `[volcurve] -1` reads, see [`SoundSet::curve_input`].
    active_since: Option<std::time::Instant>,
    /// The loudest a triggered entry has been since its trigger fired: Omsi.exe never lets
    /// such a sound get quieter while it plays (`TSound` +0x2c, reset when the trigger fires,
    /// peak hold @0x7507bc) - a door sound whose curve follows the door kept its tail.
    peak: f32,
}

pub struct SoundSet {
    sounds: Vec<RuntimeSound>,
    pub master: f32,
    /// Folder of the sound config: files of `(T.F.)` triggers resolve against it.
    dir: std::path::PathBuf,
    /// Heard from outside: non-3D sounds sit at the vehicle origin and attenuate.
    exterior: bool,
    /// The listener sits in this vehicle's interior (see [`SoundSet::set_inside`]).
    inside: bool,
    /// This set belongs to an AI vehicle (`[viewpoint]` bit 4).
    ai: bool,
    /// The player's own vehicle moves with its listener; its 3D sounds still pan and fade,
    /// but frame timing must not turn their fixed cabin positions into Doppler pitch shifts.
    listener_vehicle: bool,
    /// The listener sits in *some* vehicle's cabin right now - set every frame on every
    /// sound set, this vehicle's own and every other vehicle's alike (see
    /// [`SoundSet::set_muffled`] and [`SoundSet::lowpass_of`]).
    muffled: bool,
    /// The sound sets of the coupled parts (with the part's index among the vehicle's
    /// trailers): the rear section of an articulated bus has a `[sound]` of its own - on a
    /// pusher like the MB C2 G that is where the engine is - and plays it on the triggers
    /// and variables of the scripts it shares with the front (see [`SoundSet::update_parts`]).
    pub parts: Vec<(usize, SoundSet)>,
}

/// Say once per file that a `(T.F.)` sound cannot be found: every AI bus of a type asks for
/// the same missing announcement at every stop.
fn warn_missing_once(trigger: &str, path: &Path) {
    static SEEN: std::sync::Mutex<Option<std::collections::HashSet<std::path::PathBuf>>> =
        std::sync::Mutex::new(None);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen
        .get_or_insert_with(Default::default)
        .insert(path.to_path_buf())
    {
        log::warn!(
            "sound {} for (T.F.{trigger}) not found (further requests for it are not logged)",
            path.display()
        );
    }
}

fn curve(points: &[(f32, f32)], x: f32) -> f32 {
    if points.is_empty() {
        return 1.0;
    }
    if x <= points[0].0 {
        return points[0].1;
    }
    let last = points[points.len() - 1];
    if x >= last.0 {
        return last.1;
    }
    for w in points.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if x >= x0 && x <= x1 {
            return if x1 == x0 {
                y1
            } else {
                y0 + (y1 - y0) * (x - x0) / (x1 - x0)
            };
        }
    }
    last.1
}

impl SoundSet {
    /// The files of a sound config's fixed clips (`dir` as for [`SoundSet::new`]), for
    /// [`AudioEngine::clips_ready`].
    pub fn clip_paths(cfg: &SoundCfg, dir: &Path) -> Vec<std::path::PathBuf> {
        cfg.sounds
            .iter()
            .filter(|def| def.file.trim().parse::<i32>().is_err())
            .map(|def| omsi_cfg::resolve_path(dir, &def.file))
            .collect()
    }

    /// Load the clips of a sound config; `dir` is the directory of the config file.
    pub fn new(engine: &AudioEngine, cfg: &SoundCfg, dir: &Path) -> SoundSet {
        let sounds = cfg
            .sounds
            .iter()
            .map(|def| {
                // `[sound] N`: no fixed file, the script names one with `(T.F.trigger)`
                let dynamic = def.file.trim().parse::<i32>().is_ok();
                let path = omsi_cfg::resolve_path(dir, &def.file);
                let clip = if engine.enabled && !dynamic {
                    engine.load_clip(&path)
                } else {
                    None
                };
                RuntimeSound {
                    def: def.clone(),
                    clip,
                    voice: None,
                    held: false,
                    active_since: None,
                    peak: 0.0,
                }
            })
            .collect();
        SoundSet {
            sounds,
            master: 1.0,
            dir: dir.to_path_buf(),
            exterior: false,
            inside: false,
            ai: false,
            listener_vehicle: false,
            muffled: false,
            parts: Vec::new(),
        }
    }

    /// Where the listener is, for the `[viewpoint]` of an entry: `true` while the camera is
    /// one of this vehicle's interior views. The player's bus is told every frame; a scenery
    /// object or another vehicle is always listened to from outside.
    pub fn set_inside(&mut self, inside: bool) {
        self.inside = inside;
        for (_, p) in &mut self.parts {
            p.set_inside(inside);
        }
    }

    /// Track whether the listener travels with the player's vehicle and its coupled parts.
    pub fn set_listener_vehicle(&mut self, follows: bool) {
        self.listener_vehicle = follows;
        for (_, p) in &mut self.parts {
            p.set_listener_vehicle(follows);
        }
    }

    /// The listener sits inside *some* vehicle's cabin right now (not necessarily this one):
    /// called every frame on every sound set that exists, including AI traffic and other
    /// players' vehicles. A passing car heard from inside the player's own bus is still muffled
    /// by the player's bodywork and glass on the way in - that has nothing to do with the car's
    /// own `[viewpoint]` tags, which describe its own driver's cabin, not ours. For the
    /// player's own bus this is the same value as `set_inside`.
    pub fn set_muffled(&mut self, muffled: bool) {
        self.muffled = muffled;
        for (_, p) in &mut self.parts {
            p.set_muffled(muffled);
        }
    }

    /// Attach the sound set of coupled part `index` (built like this one: [`SoundSet::new`]
    /// for the player's bus, [`SoundSet::new_exterior`] for the others).
    pub fn add_part(&mut self, index: usize, mut part: SoundSet) {
        part.inside = self.inside;
        part.muffled = self.muffled;
        part.exterior = self.exterior;
        part.ai = self.ai;
        part.listener_vehicle = self.listener_vehicle;
        self.parts.push((index, part));
    }

    /// Per-frame update of the coupled parts' sound sets: the same variables and triggers
    /// as the leading vehicle's, each at its part's place (`part_to_world`; a part that is
    /// gone is silenced).
    pub fn update_parts(
        &mut self,
        engine: &AudioEngine,
        var: &dyn Fn(&str) -> Option<f32>,
        part_to_world: &dyn Fn(usize) -> Option<Mat4>,
        triggers: &[String],
    ) {
        for (i, p) in &mut self.parts {
            match part_to_world(*i) {
                Some(xf) => p.update(engine, var, &xf, triggers),
                None => p.stop_all(engine),
            }
        }
    }

    /// The `[viewpoint]` bits an entry must carry to be heard now: 1 from outside, 2 from
    /// inside the vehicle, 4 on an AI vehicle (the same bits as a mesh's `[viewpoint]`).
    /// Without them the EN92's exterior engine samples (`[viewpoint] 5`) played on top of
    /// the cab's own engine loop, and the rain on the roof (`[viewpoint] 2`) was heard
    /// while standing in the street.
    fn view_mask(&self) -> i32 {
        (if self.inside { 2 } else { 1 }) | (if self.ai { 4 } else { 0 })
    }

    /// How muffled an entry should sound, once the listener sits in *some* cabin (`muffled`):
    /// a foreign vehicle's sound set (`exterior`, an AI bus or another player's) is muffled
    /// then - its sounds are on the far side of the player's own bodywork and glass. The
    /// player's own bus is not: OMSI plays its entries as the `[viewpoint]` lets them through
    /// and leaves the rest to the scripts' volume curves (`Snd_OutsideVol` and the like). We
    /// muffled every entry of it not tagged as a cabin sound alone - a blinker relay tagged
    /// for inside and out was cut to a quarter below 450 Hz, heard only with a door open.
    fn lowpass_of(muffled: bool, exterior: bool) -> f32 {
        if muffled && exterior {
            // doors or the driver's window open let the outside in unfiltered
            match outside_open() {
                Some(o) => 450.0 * (1.0 + 30.0 * o.clamp(0.0, 0.5)),
                None => 450.0,
            }
        } else {
            0.0
        }
    }

    /// The share of an outside sound heard in the cab: the stock `sound_volume.osc` writes
    /// `Snd_OutsideVol` (0 with everything shut, up to 0.5 with doors or the driver's window
    /// open: "when doors are open, you can hear outside sounds louder"); a shut bus keeps a
    /// quarter, an open one all of it. Without the variable the level stays as it was.
    fn outside_gain(muffled: bool, exterior: bool) -> f32 {
        if muffled && exterior {
            match outside_open() {
                Some(o) => (0.25 + 1.5 * o.clamp(0.0, 0.5)).min(1.0),
                None => 1.0,
            }
        } else {
            1.0
        }
    }

    /// A sound set heard from outside (AI and other players' vehicles): every sound is
    /// placed at the vehicle, `[3d]` or not, so that an aircraft's engine or a passing
    /// car's horn fades with distance instead of playing at full volume everywhere. The
    /// player's own bus keeps its non-3D sounds unattenuated - the driver sits in them.
    pub fn new_exterior(engine: &AudioEngine, cfg: &SoundCfg, dir: &Path) -> SoundSet {
        let mut s = Self::new(engine, cfg, dir);
        s.exterior = true;
        s.ai = true;
        s
    }

    /// Play the sound of a `(T.F.trigger)` event: the entry listening to `trigger`
    /// plays `file` (relative to the sound folder) with its own volume and position.
    pub fn play_file_trigger(
        &mut self,
        engine: &AudioEngine,
        trigger: &str,
        file: &str,
        var: &dyn Fn(&str) -> Option<f32>,
        object_to_world: &Mat4,
    ) {
        if !engine.enabled || file.trim().is_empty() {
            return;
        }
        let path = omsi_cfg::resolve_path(&self.dir, file);
        let Some(clip) = engine.load_clip(&path) else {
            warn_missing_once(trigger, &path);
            return;
        };
        let view = self.view_mask();
        let (muffled, exterior, master) = (self.muffled, self.exterior, self.master);
        for s in self.sounds.iter_mut() {
            if !s
                .def
                .triggers
                .iter()
                .any(|d| d.eq_ignore_ascii_case(trigger))
            {
                continue;
            }
            s.active_since = Some(std::time::Instant::now());
            let Some(vol) = Self::volume(&s.def, var, view, 0.0, 1.0) else {
                continue;
            };
            let position = s
                .def
                .pos
                .map(|p| object_to_world.transform_point3(Vec3::from_array(p)));
            let params = VoiceParams {
                gain: vol * master * Self::outside_gain(muffled, exterior),
                pitch: 1.0,
                looping: false,
                position,
                doppler: !self.listener_vehicle,
                range: if s.def.range > 0.0 { s.def.range } else { 5.0 },
                lowpass_hz: Self::lowpass_of(muffled, exterior),
                important: s.def.important,
            };
            if let Some(id) = s.voice.take() {
                engine.stop(id);
            }
            s.clip = Some(clip.clone());
            s.voice = Some(engine.play(clip.clone(), params));
        }
    }

    /// Play `path` once, non-spatial, at `volume` (0..1): a sound a page asks for
    /// (`omsi.playSound`) that has no entry of its own in the `sound.cfg`.
    pub fn play_file_direct(&mut self, engine: &AudioEngine, path: &Path, volume: f32) {
        if !engine.enabled {
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
        };
        engine.play(clip, params);
    }

    /// Volume factor from the volume curves and conditions. `None` when a condition or the
    /// `[viewpoint]` silences the sound; `view` is [`SoundSet::view_mask`].
    ///
    /// The exe (`TSound` update) evaluates the conditions only for entries without a
    /// `[trigger]`: a triggered entry plays whenever its trigger fires, whatever its
    /// conditions say.
    fn volume(
        def: &SoundEntry,
        var: &dyn Fn(&str) -> Option<f32>,
        view: i32,
        active: f32,
        facing: f32,
    ) -> Option<f32> {
        // (an outside sound of the bus the camera sits in - no bit 2, the SD200's exterior
        // engine at `[viewpoint] 5` - comes into the cab through what is open, at
        // `Snd_OutsideVol`: TSound update 0x750340, played when that is over 0.01 and its
        // volume multiplied by it)
        let mut through = 1.0;
        if def.viewpoint != 0 && def.viewpoint & view == 0 {
            match outside_open() {
                Some(o) if view == 2 && def.viewpoint & 2 == 0 && o > 0.01 => through = o,
                _ => return None,
            }
        }
        if def.triggers.is_empty() && !Self::conditions_hold(def, var) {
            return None;
        }
        let mut vol = def.volume;
        for vc in &def.vol_curves {
            if let Some(x) = Self::curve_input(vc, var, active, facing) {
                vol *= curve(&vc.points, x);
            }
        }
        // DirectSound has no gain over 0 dB: OMSI turns the factor into hundredths of a
        // dB and the buffer takes at most 0, so a factor over 1 plays at 1. The MB 412D's
        // `[sound] start2.wav` carries a loop sound's lines - "44100" read as its volume -
        // and its start-up roared 44 100 times too loud.
        Some((vol * through).clamp(0.0, 1.0))
    }

    /// What a `[volcurve]` reads. The exe (`TSound` load, 0x74e408) looks the name up in the
    /// vehicle's variables and, when it is none of them, stores `StrToInt(name)` as the
    /// index; the update (0x750584) then reads a negative index from the sound's own
    /// values: -1 = seconds since the sound became active (its conditions started to hold,
    /// GetTickCount/1000), -2 = how much a `[3d]` sound with a direction faces the listener
    /// (1 without one), anything below is skipped with "Volume Variable not valid!". The
    /// LiAZ 5292's engine loops fade in with `[volcurve] -1` (0 at 1 s, 1 at 1.3 s); read
    /// as an unknown variable = 0 they were silent for ever - only the start-up and the
    /// idle of the second engine set were heard, the bus drove off in silence.
    fn curve_input(
        vc: &omsi_vehicle::VolCurve,
        var: &dyn Fn(&str) -> Option<f32>,
        active: f32,
        facing: f32,
    ) -> Option<f32> {
        match vc.variable.trim().parse::<i32>() {
            Ok(-1) => Some(active),
            Ok(-2) => Some(facing),
            Ok(n) if n < 0 => None,
            _ => Some(var(&vc.variable).unwrap_or(0.0)),
        }
    }

    fn conditions_hold(def: &SoundEntry, var: &dyn Fn(&str) -> Option<f32>) -> bool {
        def.conditions
            .iter()
            .all(|c| c.holds(var(&c.variable).unwrap_or(0.0)))
    }

    /// The playback rate of a `[loopsound]` relative to its clip, and whether it is fast
    /// enough to be played at all: the exe sets the buffer's frequency to
    /// `|pitch variable| * sample rate / reference` and silences it below DirectSound's
    /// 100 Hz minimum. 1 for a plain `[sound]`.
    fn pitch_of(def: &SoundEntry, var: &dyn Fn(&str) -> Option<f32>, clip: &Clip) -> (f32, bool) {
        if def.is_loop && def.pitch_ref != 0.0 && !def.pitch_variable.is_empty() {
            let rate = if def.sample_rate > 0.0 {
                def.sample_rate
            } else {
                clip.sample_rate as f32
            };
            let hz = var(&def.pitch_variable).unwrap_or(0.0).abs() * rate / def.pitch_ref;
            (hz / clip.sample_rate.max(1) as f32, hz >= 100.0)
        } else {
            (1.0, true)
        }
    }

    /// A triggered entry's volume this frame: never below what it has been since its
    /// trigger fired (`fired`: this frame). None (heard from the wrong view) stays None.
    fn peak_hold(peak: &mut f32, vol: Option<f32>, fired: bool) -> Option<f32> {
        if fired {
            *peak = 0.0;
        }
        let v = vol?.max(*peak);
        *peak = v;
        Some(v)
    }

    /// The triggers (lower case) whose entries read script variables in a volume curve:
    /// what [`SoundSet::update_fired`] wants the values of at the moment they fire.
    pub fn curve_triggers(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in &self.sounds {
            if !s
                .def
                .vol_curves
                .iter()
                .any(|vc| vc.variable.trim().parse::<i32>().is_err())
            {
                continue;
            }
            for t in &s.def.triggers {
                let t = t.trim().to_ascii_lowercase();
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        out
    }

    /// Per-frame update. `triggers` are the sound triggers fired by the scripts this frame.
    pub fn update(
        &mut self,
        engine: &AudioEngine,
        var: &dyn Fn(&str) -> Option<f32>,
        object_to_world: &Mat4,
        triggers: &[String],
    ) {
        self.update_fired(engine, var, object_to_world, triggers, &|_, _| None);
    }

    /// [`SoundSet::update`] with `at_fire(trigger, variable)`: a variable as it stood when
    /// the trigger fired (None: as it is now). Omsi.exe starts a trigger's sounds in the
    /// middle of the script (0x74f2e8), so the volume they start with is that of the moment:
    /// the SD200's door hits read `doorSpeed_<n>`, which the door script reverses right
    /// after firing them - read at the frame's end they were silent (#676).
    pub fn update_fired(
        &mut self,
        engine: &AudioEngine,
        var: &dyn Fn(&str) -> Option<f32>,
        object_to_world: &Mat4,
        triggers: &[String],
        at_fire: &dyn Fn(&str, &str) -> Option<f32>,
    ) {
        if !engine.enabled {
            return;
        }
        let (muffled, exterior, master, doppler) = (
            self.muffled,
            self.exterior,
            self.master,
            !self.listener_vehicle,
        );
        let view = self.view_mask();
        let world_pos = |p: Option<[f32; 3]>| {
            p.map(|p| object_to_world.transform_point3(Vec3::from_array(p)))
                .or_else(|| exterior.then(|| object_to_world.transform_point3(Vec3::ZERO)))
        };
        let range_of = |r: f32| {
            if r > 0.0 {
                r
            } else if exterior {
                40.0
            } else {
                5.0
            }
        };
        for s in self.sounds.iter_mut() {
            // How the exe plays an entry (`TSound` update, 2.2.032):
            // * with a `[trigger]`: once each time the trigger fires, from the start, never
            //   looped (a `[loopsound]` too) and without looking at its conditions;
            // * without one: looped for as long as its conditions hold and it can be heard -
            //   `[sound]` and `[loopsound]` both loop by default;
            // * without one but with `[noloop]`: once, when its conditions start to hold.
            //   The mod buses' air sounds (ECAS kneeling, the parking brake valve, the
            //   start-up chime) are written like that; looped, they hissed for ever.
            let triggered = !s.def.triggers.is_empty();
            let holds = triggered || Self::conditions_hold(&s.def, var);
            let rising = !triggered && holds && !s.held;
            s.held = holds;
            if !triggered {
                if !holds {
                    s.active_since = None;
                } else if s.active_since.is_none() {
                    s.active_since = Some(std::time::Instant::now());
                }
            }
            let Some(clip) = s.clip.clone() else { continue };
            let active = s.active_since.map_or(0.0, |t| t.elapsed().as_secs_f32());
            let facing = match (s.def.pos, s.def.dir) {
                (Some(p), Some(d)) => {
                    let at = object_to_world.transform_point3(Vec3::from_array(p));
                    let dir = object_to_world
                        .transform_vector3(Vec3::from_array(d))
                        .normalize_or_zero();
                    dir.dot((engine.listener_position() - at).normalize_or_zero())
                }
                _ => 1.0,
            };
            let fired_by = if triggered {
                triggers.iter().find(|t| {
                    s.def
                        .triggers
                        .iter()
                        .any(|d| d.trim().eq_ignore_ascii_case(t))
                })
            } else {
                None
            };
            let fired = fired_by.is_some();
            let mut vol = match fired_by {
                Some(t) => Self::volume(
                    &s.def,
                    &|n| at_fire(t, n).or_else(|| var(n)),
                    view,
                    active,
                    facing,
                ),
                None => Self::volume(&s.def, var, view, active, facing),
            };
            if triggered {
                vol = Self::peak_hold(&mut s.peak, vol, fired);
            }
            let (pitch, fast_enough) = Self::pitch_of(&s.def, var, &clip);
            let audible = vol.map(|v| v > 0.001).unwrap_or(false) && fast_enough;
            let params = |looping: bool| VoiceParams {
                gain: vol.unwrap_or(0.0) * master * Self::outside_gain(muffled, exterior),
                pitch: pitch.max(0.001),
                looping,
                position: world_pos(s.def.pos),
                doppler,
                range: range_of(s.def.range),
                lowpass_hz: Self::lowpass_of(muffled, exterior),
                important: s.def.important,
            };
            if !triggered && !s.def.no_loop {
                let params = params(true);
                match (s.voice, audible) {
                    (Some(id), true) => {
                        if engine.is_playing(id) {
                            engine.set_params(id, params);
                        } else {
                            s.voice = Some(engine.play(clip, params));
                        }
                    }
                    (Some(id), false) => {
                        engine.stop(id);
                        s.voice = None;
                    }
                    (None, true) => s.voice = Some(engine.play(clip, params)),
                    (None, false) => {}
                }
                continue;
            }
            // one-shot: started by its trigger or by its conditions starting to hold; the
            // volume follows the curves while it plays (a triggered one only gets louder)
            let params = params(false);
            if (fired || rising) && audible {
                if let Some(id) = s.voice {
                    if s.def.only_one && engine.is_playing(id) {
                        engine.set_params(id, params);
                        continue;
                    }
                    engine.stop(id);
                }
                s.voice = Some(engine.play(clip, params));
            } else if let Some(id) = s.voice {
                if engine.is_playing(id) {
                    engine.set_params(id, params);
                } else {
                    s.voice = None;
                }
            }
        }
    }

    /// How many `[sound]`/`[loopsound]` entries the configuration has.
    pub fn len(&self) -> usize {
        self.sounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }

    /// The sounds playing right now: (file, gain at the listener, pitch) - for the logs.
    pub fn playing(&self, engine: &AudioEngine) -> Vec<(String, f32, f32)> {
        self.sounds
            .iter()
            .filter_map(|s| {
                s.voice
                    .and_then(|id| engine.voice_state(id))
                    .map(|(p, heard)| (s.def.file.clone(), heard, p.pitch))
            })
            .collect()
    }

    /// One line per entry of the configuration, saying whether it is heard and why not
    /// (`OMSI_DEBUG_SOUND`): the only way to see which of a bus's hundred sounds the
    /// rewrite never reaches - a missing clip, a condition on a variable nobody feeds, a
    /// volume curve that stays at zero or a `[viewpoint]` the camera is not in.
    pub fn report(&self, engine: &AudioEngine, var: &dyn Fn(&str) -> Option<f32>) -> Vec<String> {
        let view = self.view_mask();
        let mut out = Vec::new();
        for s in &self.sounds {
            let file = s.def.file.trim();
            let mut why = String::new();
            if s.clip.is_none() {
                why = if file.parse::<i32>().is_ok() {
                    "waits for a (T.F.) file".into()
                } else {
                    "no clip".into()
                };
            } else if s.def.viewpoint != 0
                && s.def.viewpoint & view == 0
                && !(view == 2
                    && s.def.viewpoint & 2 == 0
                    && outside_open().is_some_and(|o| o > 0.01))
            {
                why = format!("viewpoint {} (listener {view})", s.def.viewpoint);
            } else if let Some(c) = s
                .def
                .conditions
                .iter()
                .find(|c| !c.holds(var(&c.variable).unwrap_or(0.0)))
            {
                why = format!("condition {} = {:?}", c.variable, var(&c.variable));
            } else if let Some(vc) = s.def.vol_curves.iter().find(|vc| {
                let active = s.active_since.map_or(0.0, |t| t.elapsed().as_secs_f32());
                Self::curve_input(vc, var, active, 1.0)
                    .is_some_and(|x| curve(&vc.points, x) <= 0.001)
            }) {
                why = format!("volcurve {} = {:?}", vc.variable, var(&vc.variable));
            }
            let playing = s.voice.and_then(|id| engine.voice_state(id));
            match (playing, why.is_empty()) {
                (Some((p, heard)), _) => {
                    out.push(format!("{file}: {:.3} heard, pitch {:.2}", heard, p.pitch))
                }
                (None, true) => out.push(format!("{file}: ready, silent")),
                (None, false) => out.push(format!("{file}: off - {why}")),
            }
        }
        out
    }

    pub fn stop_all(&mut self, engine: &AudioEngine) {
        for s in self.sounds.iter_mut() {
            if let Some(id) = s.voice.take() {
                engine.stop(id);
            }
        }
        for (_, p) in &mut self.parts {
            p.stop_all(engine);
        }
    }
}

/// `Snd_OutsideVol` of the bus the listener sits in (bits of an f32; NaN = none).
static OUTSIDE_OPEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x7fc0_0000);

/// Set how open the listener's bus is to the outside (`Snd_OutsideVol`), or `None`.
pub fn set_outside_open(v: Option<f32>) {
    OUTSIDE_OPEN.store(
        v.filter(|x| x.is_finite()).unwrap_or(f32::NAN).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn outside_open() -> Option<f32> {
    let v = f32::from_bits(OUTSIDE_OPEN.load(std::sync::atomic::Ordering::Relaxed));
    (!v.is_nan()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_doors_let_the_outside_in() {
        set_outside_open(None);
        assert_eq!(
            SoundSet::outside_gain(true, true),
            1.0,
            "no variable: as before"
        );
        set_outside_open(Some(0.0));
        assert_eq!(SoundSet::outside_gain(true, true), 0.25, "shut: a quarter");
        assert_eq!(
            SoundSet::outside_gain(true, false),
            1.0,
            "the own bus's sounds are its own"
        );
        assert_eq!(SoundSet::lowpass_of(true, false), 0.0);
        assert_eq!(SoundSet::outside_gain(false, true), 1.0, "standing outside");
        set_outside_open(Some(0.5));
        assert_eq!(
            SoundSet::outside_gain(true, true),
            1.0,
            "doors open: all of it"
        );
        assert!(SoundSet::lowpass_of(true, true) > 5000.0);
        // (in the same test: the variable is one for all) the own bus's outside-only
        // entry, `[viewpoint] 5`, heard from the cab at `Snd_OutsideVol` (TSound update
        // 0x750340), not at all with everything shut
        let engine = SoundEntry {
            volume: 0.8,
            viewpoint: 5,
            ..Default::default()
        };
        let none = |_: &str| None;
        set_outside_open(Some(0.0));
        assert_eq!(SoundSet::volume(&engine, &none, 2, 0.0, 1.0), None);
        set_outside_open(Some(0.5));
        assert_eq!(SoundSet::volume(&engine, &none, 2, 0.0, 1.0), Some(0.4));
        assert_eq!(
            SoundSet::volume(&engine, &none, 1, 0.0, 1.0),
            Some(0.8),
            "outside: as it is"
        );
        let outside = SoundEntry {
            volume: 0.8,
            viewpoint: 1,
            ..Default::default()
        };
        assert_eq!(SoundSet::volume(&outside, &none, 2, 0.0, 1.0), Some(0.4));
        assert_eq!(
            SoundSet::volume(&outside, &none, 2 | 4, 0.0, 1.0),
            None,
            "not an AI bus's"
        );
        let cab = SoundEntry {
            volume: 0.8,
            viewpoint: 2,
            ..Default::default()
        };
        assert_eq!(
            SoundSet::volume(&cab, &none, 1, 0.0, 1.0),
            None,
            "a cab sound stays in"
        );
        set_outside_open(None);
        assert_eq!(SoundSet::volume(&engine, &none, 2, 0.0, 1.0), None);
    }

    /// The SD200's door hit: `[volcurve] doorSpeed_0` from 1 at -1 down to 0 at -0.5,
    /// fired while the door still closes at -1 m/s; by the frame's end the script has turned
    /// the speed round. The volume is the one of the moment it fired (#676).
    #[test]
    fn a_triggered_sound_reads_its_curve_when_it_fires() {
        let hit = SoundEntry {
            volume: 1.0,
            triggers: vec!["ev_doorhitclose_0".into()],
            vol_curves: vec![omsi_vehicle::VolCurve {
                variable: "doorSpeed_0".into(),
                points: vec![(-1.0, 1.0), (-0.5, 0.0)],
            }],
            ..Default::default()
        };
        let now = |n: &str| (n == "doorSpeed_0").then_some(0.8);
        let at_fire =
            |t: &str, n: &str| (t == "ev_doorhitclose_0" && n == "doorSpeed_0").then_some(-1.0);
        set_outside_open(None);
        assert_eq!(
            SoundSet::volume(&hit, &now, 2, 0.0, 1.0),
            Some(0.0),
            "read at the frame's end: silent"
        );
        let fired = |n: &str| at_fire("ev_doorhitclose_0", n).or_else(|| now(n));
        assert_eq!(SoundSet::volume(&hit, &fired, 2, 0.0, 1.0), Some(1.0));
        let set = SoundSet {
            sounds: vec![RuntimeSound {
                def: hit,
                clip: None,
                voice: None,
                held: false,
                active_since: None,
                peak: 0.0,
            }],
            master: 1.0,
            dir: Default::default(),
            exterior: false,
            inside: true,
            ai: false,
            listener_vehicle: true,
            muffled: false,
            parts: Vec::new(),
        };
        assert_eq!(set.curve_triggers(), vec!["ev_doorhitclose_0".to_string()]);
    }

    #[test]
    fn a_triggered_sound_keeps_its_peak_while_it_plays() {
        // a door sound whose volcurve follows the door: fired with the door at 1, the door
        // closed the next frame - Omsi.exe holds the peak (0x7507bc)
        let mut peak = 0.7;
        assert_eq!(SoundSet::peak_hold(&mut peak, Some(1.0), true), Some(1.0));
        assert_eq!(SoundSet::peak_hold(&mut peak, Some(0.0), false), Some(1.0));
        assert_eq!(
            SoundSet::peak_hold(&mut peak, None, false),
            None,
            "wrong view: silent"
        );
        // fired again quieter: the old peak is forgotten
        assert_eq!(SoundSet::peak_hold(&mut peak, Some(0.3), true), Some(0.3));
    }
}
