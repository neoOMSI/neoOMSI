//! The runtime of a sound configuration attached to an object with script variables: which
//! entries play, how they are updated per frame and how the coupled parts are driven. The
//! per-entry decision lives in [`crate::runtime::sound`].

use crate::clock::Clock;
use crate::engine::Playback;
use crate::runtime::event::SoundEvent;
use crate::runtime::level::SOUND_MAXCOUNT;
use crate::runtime::sound::{EntryCtx, OnlyOne, RuntimeSound, SoundState};


use glam::{Mat4, Vec3};
use ::legacy_vehicle::SoundCfg;
use std::path::Path;

pub use crate::runtime::outside::set_outside_open;

pub struct SoundSet {
    pub(super) sounds: Vec<RuntimeSound>,
    /// Time source for sound activity and the inside/outside blend: the wall clock in
    /// normal playback, a manual clock in tests and the offline renderer.
    pub(super) clock: Clock,
    pub master: f32,
    pub(super) bus: crate::voice::bus::Bus,
    pub(super) file_bus: crate::voice::bus::Bus,
    pub(super) announcement_reverb: f32,
    /// Folder of the sound config: files of `(T.F.)` triggers resolve against it.
    pub(super) dir: std::path::PathBuf,
    /// Heard from outside: non-3D sounds sit at the vehicle origin and attenuate.
    pub(super) exterior: bool,
    /// The listener sits in this vehicle's interior (see [`SoundSet::set_inside`]).
    pub(super) inside: bool,
    /// This set belongs to an AI vehicle (`[viewpoint]` bit 4).
    pub(super) ai: bool,
    /// The player's own vehicle moves with its listener; its 3D sounds still pan and fade,
    /// but frame timing must not turn their fixed cabin positions into Doppler pitch shifts.
    pub(super) listener_vehicle: bool,
    /// The listener sits in *some* vehicle's cabin right now - set every frame on every
    /// sound set, this vehicle's own and every other vehicle's alike.
    pub(super) muffled: bool,
    /// Smoothed 0..1 follow-ups of `inside` and `muffled`: the bodywork does not switch,
    /// the sound is faded between the two sides over a fraction of a second.
    inside_blend: f32,
    muffled_blend: f32,
    /// The vehicle's hull as a box in its own space (centre, half size): where the
    /// listener stands against it - not a flag or a timer - decides how much bodywork lies
    /// between the listener and the sounds (see [`SoundSet::inside_factor`]).
    pub(super) hull: Option<([f32; 3], [f32; 3])>,
    pub(super) cabin_connected: bool,
    /// What the leading vehicle's hull gave this frame, for the coupled parts.
    pub(super) hull_h: f32,
    pub(super) hull_override: Option<f32>,
    blend_at: Option<std::time::Instant>,
    /// The sound sets of the coupled parts (with the part's index among the vehicle's
    /// trailers): the rear section of an articulated bus has a `[sound]` of its own - on a
    /// pusher like the MB C2 G that is where the engine is - and plays it on the triggers
    /// and variables of the scripts it shares with the front (see [`SoundSet::update_parts`]).
    pub parts: Vec<(usize, SoundSet)>,
    /// The central `[onlyone]` registry of this set (file -> running voice). Rebuilt and
    /// pruned every frame (see [`SoundSet::update_frame`]).
    pub(super) only_one: OnlyOne,
}

impl SoundSet {
    /// The files of a sound config's fixed clips (`dir` as for [`SoundSet::new`]), for
    /// [`crate::engine::AudioEngine::clips_ready`].
    pub fn clip_paths(cfg: &SoundCfg, dir: &Path) -> Vec<std::path::PathBuf> {
        cfg.sounds
            .iter()
            .filter(|def| def.file.trim().parse::<i32>().is_err())
            .map(|def| ::mods::resolve_path(dir, &def.file))
            .collect()
    }

    /// Load the clips of a sound config; `dir` is the directory of the config file.
    pub fn new(engine: &dyn Playback, cfg: &SoundCfg, dir: &Path) -> SoundSet {
        let sounds = cfg
            .sounds
            .iter()
            .map(|def| {
                // `[sound] N`: no fixed file, the script names one with `(T.F.trigger)`;
                // a fixed file is read lazily on the first update (see `RuntimeSound`)
                let dynamic = def.file.trim().parse::<i32>().is_ok();
                let path = if engine.enabled() && !dynamic {
                    Some(::mods::resolve_path(dir, &def.file))
                } else {
                    None
                };
                RuntimeSound::new(def.clone(), path)
            })
            .collect();
        SoundSet {
            sounds,
            clock: engine.clock(),
            master: 1.0,
            bus: crate::voice::bus::Bus::Vehicle,
            // The fallback bus of a `(T.F.)` dynamic file. Announcements are routed to
            // `Bus::Announcement` per file (see `play_file_trigger`); everything else - the
            // C2 gearbox and every other scripted dynamic sound - stays on the vehicle.
            file_bus: crate::voice::bus::Bus::Vehicle,
            announcement_reverb: ::legacy_config::env::var("OMSI_ANNOUNCEMENT_REVERB").ok()
                .and_then(|s| s.parse::<f32>().ok()).filter(|v| v.is_finite())
                .unwrap_or(0.25).clamp(0.0, 0.5),
            dir: dir.to_path_buf(),
            exterior: false,
            inside: false,
            ai: false,
            listener_vehicle: false,
            muffled: false,
            inside_blend: 0.0,
            muffled_blend: 0.0,
            hull: None,
            cabin_connected: false,
            hull_h: 0.0,
            hull_override: None,
            blend_at: None,
            parts: Vec::new(),
            only_one: OnlyOne::new(),
        }
    }

    /// The logical state of entry `index` (see [`SoundState`]).
    pub fn state(&self, index: usize) -> Option<SoundState> {
        self.sounds.get(index).map(|s| s.state())
    }

    /// Move the inside / muffled blends towards their targets (about 0.45 s for the whole
    /// way) and return them eased.
    pub(super) fn advance_blend(&mut self) -> (f32, f32) {
        let now = self.clock.now();
        // A freshly loaded set must start on the listener's current side. Starting
        // its muffling at zero caused a loud exterior burst lasting several frames.
        let Some(previous) = self.blend_at else {
            self.blend_at = Some(now);
            self.inside_blend = if self.inside { 1.0 } else { 0.0 };
            self.muffled_blend = if self.muffled { 1.0 } else { 0.0 };
            return (self.inside_blend, self.muffled_blend);
        };
        let dt = now.saturating_duration_since(previous).as_secs_f32().min(0.1);
        self.blend_at = Some(now);
        let step = dt / 0.45;
        let toward = |cur: f32, target: bool| {
            let t = if target { 1.0 } else { 0.0 };
            if cur < t {
                (cur + step).min(t)
            } else {
                (cur - step).max(t)
            }
        };
        self.inside_blend = toward(self.inside_blend, self.inside);
        self.muffled_blend = toward(self.muffled_blend, self.muffled);
        let ease = |b: f32| b * b * (3.0 - 2.0 * b);
        (ease(self.inside_blend), ease(self.muffled_blend))
    }

    /// The hull of the vehicle from its `[boundingbox]` (width, length, height, centre).
    pub fn set_hull(&mut self, bb: Option<[f32; 6]>) {
        self.hull = bb.map(|b| {
            (
                [b[3], b[4], b[5]],
                [b[0] * 0.5 + 0.05, b[1] * 0.5 + 0.05, b[2] * 0.5 + 0.05],
            )
        });
    }

    /// How much the listener is inside the bodywork, 0..1, from where they stand: across
    /// the hull's wall it runs over half a metre, so walking through the door the sound changes
    /// with every step - and stands still when they do. Without a hull, the eased flag.
    pub(super) fn inside_factor(&mut self, eased: f32, object_to_world: &Mat4, listener: Vec3) -> f32 {
        let h = if self.inside {
            // A declared cab/passenger view is already inside, even on the first frame
            // before the engine receives the spawned vehicle's new camera position.
            1.0
        } else if let Some(h) = self.hull_override {
            h
        } else if let Some((c, half)) = self.hull {
            super::cabin::hull_factor((c, half), *object_to_world, listener)
        } else {
            eased
        };
        self.hull_h = h;
        h
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
        part.set_bus(self.bus);
        part.set_file_bus(self.file_bus);
        part.set_announcement_reverb(self.announcement_reverb);
        part.inside = self.inside;
        part.muffled = self.muffled;
        part.inside_blend = self.inside_blend;
        part.muffled_blend = self.muffled_blend;
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
        engine: &dyn Playback,
        var: &dyn Fn(&str) -> Option<f32>,
        part_to_world: &dyn Fn(usize) -> Option<Mat4>,
        triggers: &[String],
    ) {
        for (i, p) in &mut self.parts {
            match part_to_world(*i) {
                Some(xf) => {
                    if p.hull.is_none() {
                        p.hull_override = Some(self.hull_h);
                    }
                    p.update(engine, var, &xf, triggers)
                }
                None => p.stop_all(engine),
            }
        }
    }

    /// A sound set heard from outside (AI and other players' vehicles): every sound is
    /// placed at the vehicle, `[3d]` or not, so that an aircraft's engine or a passing
    /// car's horn fades with distance instead of playing at full volume everywhere. The
    /// player's own bus keeps its non-3D sounds unattenuated - the driver sits in them.
    pub fn new_exterior(engine: &dyn Playback, cfg: &SoundCfg, dir: &Path) -> SoundSet {
        let mut s = Self::new(engine, cfg, dir);
        s.exterior = true;
        s.ai = true;
        s
    }

    /// A world/scenery emitter, heard through the listener's cabin when muffled.
    /// It is spatial/exterior like traffic, but keeps scenery's non-AI viewpoint rules.
    pub fn new_world(engine: &dyn Playback, cfg: &SoundCfg, dir: &Path) -> SoundSet {
        let mut s = Self::new(engine, cfg, dir);
        s.exterior = true;
        s.set_bus(crate::Bus::Ambience);
        s.set_file_bus(crate::Bus::Ambience);
        s
    }

    /// Per-frame update. `triggers` are the sound triggers fired by the scripts this frame.
    pub fn update(
        &mut self,
        engine: &dyn Playback,
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
        engine: &dyn Playback,
        var: &dyn Fn(&str) -> Option<f32>,
        object_to_world: &Mat4,
        triggers: &[String],
        at_fire: &dyn Fn(&str, &str) -> Option<f32>,
    ) {
        self.update_frame(engine, var, object_to_world, triggers, at_fire);
    }

    /// One frame's **ordered event stream** (see [`crate::runtime::event`]): the normal
    /// triggers and the `(T.F.)` file triggers are no longer two lists but events of one
    /// stream, each tagged with its source, sequence and firing time. `slots` maps a script
    /// variable name to its index in an event's variable snapshot, so a triggered entry
    /// reads the value of the moment it fired. Existing callers keep using
    /// [`SoundSet::update_fired`] + [`SoundSet::play_file_trigger`]; this is the unified
    /// entry point the callers migrate to.
    ///
    /// Repeated events survive as separate entries. The current rule (trigger restarts an
    /// entry once per frame; a file trigger restarts it each time) is unchanged, so the
    /// behavior of two identical firings is exactly what it was - the stream only makes it
    /// observable. The exact OMSI rule is OPEN (behavior doc section 4).
    ///
    /// Note (current playback rule): all normal triggers are applied first in the frame-wide
    /// pass, then the file events, so the stream order does not change what plays for a file
    /// event and a normal trigger in the same frame (see [`crate::runtime::event`]).
    pub fn update_events(
        &mut self,
        engine: &dyn Playback,
        var: &dyn Fn(&str) -> Option<f32>,
        object_to_world: &Mat4,
        events: &[SoundEvent],
        slots: &dyn Fn(&str) -> Option<usize>,
    ) {
        if !engine.enabled() {
            return;
        }
        let triggers: Vec<String> = events
            .iter()
            .filter(|e| !e.is_file())
            .map(|e| e.trigger.clone())
            .collect();
        // the value of the moment it fired (the last snapshot for that trigger name,
        // `TSound` update 0x750584); falls back to the current value in `update_frame`
        let at_fire = |t: &str, n: &str| -> Option<f32> {
            let slot = slots(n)?;
            events
                .iter()
                .rev()
                .find(|e| e.trigger.eq_ignore_ascii_case(t) && e.vars.is_some())
                .and_then(|e| e.vars.as_ref()?.get(slot).copied())
        };
        self.update_frame(engine, var, object_to_world, &triggers, &at_fire);
        for e in events.iter().filter(|e| e.is_file()) {
            if let Some(file) = &e.file {
                let at_file = |n: &str| slots(n).and_then(|slot| e.vars.as_ref()?.get(slot).copied()).or_else(|| var(n));
                self.play_file_trigger(engine, &e.trigger, file, &at_file, object_to_world);
            }
        }
    }

    /// The shared body of [`SoundSet::update`], [`SoundSet::update_fired`] and
    /// [`SoundSet::update_events`]: evaluate every entry against the frame's variables.
    fn update_frame(
        &mut self,
        engine: &dyn Playback,
        var: &dyn Fn(&str) -> Option<f32>,
        object_to_world: &Mat4,
        triggers: &[String],
        at_fire: &dyn Fn(&str, &str) -> Option<f32>,
    ) {
        if !engine.enabled() {
            return;
        }
        let (eased, muffled) = self.advance_blend();
        let listener = engine.listener_position();
        let inside = self.inside_factor(eased, object_to_world, listener);
        let (exterior, master, doppler) = (self.exterior, self.master, !self.listener_vehicle);
        let ai = self.ai;
        let now = self.clock.now();
        let cx = EntryCtx {
            bus: self.bus,
            announcement_reverb: self.announcement_reverb,
            var,
            at_fire,
            object_to_world,
            listener,
            triggers,
            ai,
            exterior,
            inside,
            muffled,
            master,
            doppler,
            now,
        };
        // The `[onlyone]` registry is per set: drop ids the mixer has already ended, then
        // let every entry of this frame consult and update it.
        let mut only_one = std::mem::take(&mut self.only_one);
        only_one.retain(|_, id| engine.is_playing(*id));
        // The runtime's OMSI admission budget (`[sound_maxcount]`), counted over this set's
        // running voices. Past it an entry keeps running but no new voice starts; the mixer's
        // separate `MAX_VOICES` virtualization is invisible here.
        let mut used = self
            .sounds
            .iter()
            .filter(|s| s.voice.is_some_and(|id| engine.is_playing(id)))
            .count();
        for s in self.sounds.iter_mut() {
            let before = s.voice.is_some_and(|id| engine.is_playing(id));
            s.update(engine, &cx, used < SOUND_MAXCOUNT, &mut only_one);
            let after = s.voice.is_some_and(|id| engine.is_playing(id));
            used = used
                .saturating_add(usize::from(after))
                .saturating_sub(usize::from(before));
        }
        self.only_one = only_one;
    }

    pub fn stop_all(&mut self, engine: &dyn Playback) {
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
