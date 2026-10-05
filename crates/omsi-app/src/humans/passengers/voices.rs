use super::*;

impl Humans {
    /// `limited`: said only when the same file has not been said for 10 s (greetings and
    /// complaints; the ticket asked for, "thanks" and the missing change always are).
    pub(in crate::humans) fn say_ex(&mut self, i: usize, name: &str, limited: bool) -> bool {
        // the player may have silenced them (settings), all but the ticket they ask for
        match self.voices {
            2 => return false,
            1 if !name.starts_with("Ticket_") => return false,
            _ => {}
        }
        // Greetings and complaints: one at a time for the whole bus. OMSI only keeps
        // the same file from being said twice within 10 s, and with a dozen people
        // boarding every other one said hello - the saloon never stopped talking, which
        // is not how the original sounds: a few words now and then.
        if limited
            && self.time - self.voice.last_chat < CHAT_PAUSE
            && self.time >= self.voice.last_chat
        {
            return false;
        }
        // (without a `[voicepath]` the pack's own folder: Berlin_1 and Berlin_86 carry the
        // voices themselves and name no path; the later packs point at theirs)
        let Some(base) = self.tickets.as_ref().and_then(|t| match &t.voice_path {
            Some(vp) if !vp.trim().is_empty() => {
                Some(omsi_cfg::resolve_path(&self.root, vp.trim()))
            }
            _ => t.path.parent().map(|p| p.to_path_buf()),
        }) else {
            return false;
        };
        let voice = self.people[i].ty.def.voice.trim().to_string();
        if voice.is_empty() {
            return false;
        }
        let dir = omsi_cfg::resolve_path(&base, &voice);
        let primary = omsi_cfg::resolve_path(&dir, &format!("{name}.wav"));
        let path = if omsi_cfg::vfs::is_file(&primary) {
            primary
        } else if name.starts_with("TooBad_") {
            // Some map-specific ticket packs (for example Bowdenham V5) define the
            // passenger types but ship no driving-complaint samples.  Use OMSI's
            // standard Berlin voices of the same type when they are installed, so the
            // simulation feedback is not silently lost with such content.
            let standard = omsi_cfg::resolve_path(&self.root, "TicketPacks\\Berlin_86");
            let fallback = omsi_cfg::resolve_path(
                &omsi_cfg::resolve_path(&standard, &voice),
                &format!("{name}.wav"),
            );
            if !omsi_cfg::vfs::is_file(&fallback) {
                return false;
            }
            if debug_pax() {
                log::info!(
                    "pax voice {name}: {} is missing; using {}",
                    primary.display(),
                    fallback.display()
                );
            }
            fallback
        } else {
            return false;
        };
        if limited {
            if let Some(&t) = self.voice.voice_said.get(&path) {
                if self.time - t < 10.0 && self.time >= t {
                    return false;
                }
            }
        }
        self.voice.voice_said.insert(path.clone(), self.time);
        if limited {
            self.voice.last_chat = self.time;
        }
        if debug_pax() {
            log::info!(
                "t={:.1} pax {} says {name}",
                self.time,
                self.people[i].label()
            );
        }
        self.voice.voice_lines.push(VoiceLine {
            position: self.people[i].position + DVec3::new(0.0, 0.0, 1.6),
            path,
        });
        true
    }

    /// Lines passengers said since the last call (the app plays them where they stand).
    pub fn take_voice_lines(&mut self) -> Vec<VoiceLine> {
        std::mem::take(&mut self.voice.voice_lines)
    }
}

pub(in crate::humans) struct PassengerVoices {
    /// What passengers said since the app last collected it (see `take_voice_lines`).
    pub(in crate::humans) voice_lines: Vec<VoiceLine>,
    /// When each voice file was last said (seconds of `time`): OMSI keeps such a list
    /// and says a greeting or a complaint only when that
    /// very file has not been heard for 10 s - without it every boarding passenger said
    /// "Hallo" one after the other.
    pub(in crate::humans) voice_said: HashMap<std::path::PathBuf, f64>,
    /// When anybody last greeted or complained (seconds of `time`).
    pub(in crate::humans) last_chat: f64,
}
