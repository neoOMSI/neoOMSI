//! `Inputs/keyboard.cfg`, `Inputs/gamectrler.cfg`, `Inputs/*.kyb` (unit `mc_input`).

use ::legacy_config::CfgFile;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeyBinding {
    pub action: String,
    /// DirectInput scan code
    pub scan_code: i32,
    /// The entry's third value, as Omsi.exe reads it (0x6478d0): [`KEY_HOLD`], [`KEY_SHIFT`],
    /// [`KEY_CTRL`] (and our [`KEY_ALT`]). Kept whole, so that it is written back as read.
    pub modifier: i32,
}

/// The action is told the key's state every frame, not only when it changes (the key list
/// marks it with " *"): the throttle, the brake, the steering. No part of the key chord.
pub const KEY_HOLD: i32 = 1;
/// Held with Shift (OMSI shows "Shift + "; its Standlicht is Shift+L, bit 2).
pub const KEY_SHIFT: i32 = 2;
/// Held with Ctrl (Ctrl+Q ends the game: `exit` 16 / 4).
pub const KEY_CTRL: i32 = 4;
/// Held with Alt: Omsi.exe has no Alt (it reads the three bits above only); ours, which it
/// leaves alone.
pub const KEY_ALT: i32 = 8;

impl KeyBinding {
    /// The keys held with it (Shift, Ctrl, Alt bits), without [`KEY_HOLD`].
    pub fn chord(&self) -> i32 {
        self.modifier & (KEY_SHIFT | KEY_CTRL | KEY_ALT)
    }

    /// Whether the modifier keys held (`held`, [`chord`] bits) make its chord: Shift and
    /// Ctrl as they are, as Omsi.exe compares them (0x6466b8); Alt only when it asks for
    /// Alt (Omsi.exe has none, and fires its keys whether Alt is held or not).
    pub fn matches(&self, held: i32) -> bool {
        let sc = KEY_SHIFT | KEY_CTRL;
        self.chord() & sc == held & sc && (self.chord() & KEY_ALT == 0 || held & KEY_ALT != 0)
    }
}

/// The chord bits of the modifier keys held.
pub fn chord(shift: bool, ctrl: bool, alt: bool) -> i32 {
    (shift as i32) * KEY_SHIFT | (ctrl as i32) * KEY_CTRL | (alt as i32) * KEY_ALT
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeyboardCfg {
    pub game: Vec<KeyBinding>,
    pub vehicles: Vec<KeyBinding>,
}

impl KeyboardCfg {
    pub fn load(path: &Path) -> Result<KeyboardCfg, ::legacy_config::CfgError> {
        let f = CfgFile::read(path)?;
        let mut k = KeyboardCfg::default();
        let mut section = 0;
        let mut r = f.reader();
        while let Some(kw) = r.next_keyword() {
            match kw.as_str() {
                "game" => section = 0,
                "vehicles" => section = 1,
                "entry" => {
                    let b = KeyBinding {
                        action: r.str().to_string(),
                        scan_code: r.i32(),
                        modifier: r.i32(),
                    };
                    if section == 0 {
                        k.game.push(b);
                    } else {
                        k.vehicles.push(b);
                    }
                }
                _ => {}
            }
        }
        Ok(k)
    }

    /// The keys the game adds to the file's (not written back by `save`).
    pub fn with_game_defaults(mut self) -> Self {
        // The IBIS's next stop with its announcement (`IBIS_vor`) has no key in OMSI's own
        // file: only the mouse on the IBIS reached it. Q (scan code 16, no modifier: Ctrl+Q
        // ends the game, Shift+Q is the microphone) gives it one where Q is still free.
        let q_taken = self
            .game
            .iter()
            .chain(self.vehicles.iter())
            .any(|b| b.scan_code == 16 && b.chord() == 0);
        if !q_taken
            && !self
            .vehicles
            .iter()
            .any(|b| b.action.eq_ignore_ascii_case("IBIS_vor"))
        {
            self.vehicles.push(KeyBinding {
                action: "IBIS_vor".into(),
                scan_code: 16,
                modifier: 0,
            });
        }
        // the LAN chat's keys, which the player can move like any other (#130): V shows
        // and hides the chat (scan code 47, bound to nothing in OMSI's file), and the line
        // to write in opens on '/' (53) - unless the file gives that key to something else:
        // OMSI's own gives it to the manual gearbox's `kw_s_minus` (a German keyboard's '-',
        // with `kw_s_plus` on 'Ü' next to it), so the chat took the gear down away in a LAN
        // game. Then it is the key left of 1 ('`', '^'; 41), or '\' / '#' (43).
        let taken = |k: &KeyboardCfg, sc: i32| {
            k.game.iter().chain(k.vehicles.iter()).any(|b| {
                b.scan_code == sc
                    && b.chord() == 0
                    && !b.action.to_ascii_lowercase().starts_with("chat_")
            })
        };
        let open_key = [53, 41, 43]
            .into_iter()
            .find(|&sc| !taken(&self, sc))
            .unwrap_or(53);
        // (a file written while the default was '/' whatever the gearbox had: moved off it)
        if let Some(b) = self
            .game
            .iter_mut()
            .find(|b| b.action.eq_ignore_ascii_case("chat_open"))
        {
            if b.scan_code == 53 && b.chord() == 0 && open_key != 53 {
                b.scan_code = open_key;
            }
        }
        for (action, scan_code) in [("chat_toggle", 47), ("chat_open", open_key)] {
            if !self
                .game
                .iter()
                .any(|b| b.action.eq_ignore_ascii_case(action))
            {
                self.game.push(KeyBinding {
                    action: action.into(),
                    scan_code,
                    modifier: 0,
                });
            }
        }
        for action in ["blinker_left_toggle", "blinker_right_toggle"] {
            if !self
                .vehicles
                .iter()
                .any(|b| b.action.eq_ignore_ascii_case(action))
            {
                self.vehicles.push(KeyBinding {
                    action: action.into(),
                    scan_code: 0,
                    modifier: 0,
                });
            }
        }
        // The game's own keys that are not in OMSI's file (or that a short file lacks): in the
        // same editable list as the rest, so the player can move or unbind each. An entry in
        // the file (an unbound one too) wins.
        let mut keys: Vec<(String, i32, i32)> = [
            ("sim_pause", 25, 0),
            ("quicksave", 31, KEY_CTRL),
            ("screenshot", 25, KEY_SHIFT | KEY_CTRL),
            ("toggel_mouse_ctrl", 24, 0),
            ("view_set_driver", 59, 0),
            ("view_set_passenger", 60, 0),
            ("view_set_outside", 61, 0),
            ("view_set_map", 62, 0),
            ("view_set_schedule", 210, KEY_HOLD),
            ("view_toggle_informationdisplay", 21, KEY_SHIFT),
            ("toggle_fullscreen", 28, KEY_ALT),
            ("toggle_editor", 18, KEY_SHIFT | KEY_CTRL),
            ("gear_up", 200, KEY_CTRL),
            ("gear_down", 208, KEY_CTRL),
            ("indicator_left", 44, 0),
            ("indicator_hazard", 45, 0),
            ("indicator_right", 46, 0),
            ("saloon_lights", 23, 0),
            ("bus_startup", 22, KEY_SHIFT),
            ("radio_next", 19, KEY_SHIFT),
            ("voice_talk", 58, 0),
            ("voice_radio", 58, KEY_SHIFT),
            ("toggle_city_map", 50, KEY_SHIFT),
            ("show_position", 87, KEY_CTRL),
            ("save_personnel", 67, 0),
            ("open_mainmenue", 1, 0),
            ("tutorial_next", 28, 0),
            ("tutorial_back", 201, 0),
            ("tutorial_toggle", 20, KEY_CTRL),
            ("navigator_close", 1, 0),
            ("menu_exit", 1, 0),
        ]
            .into_iter()
            .map(|(a, s, m)| (a.to_string(), s, m))
            .collect();
        // Shift+1..9: the doors, front to back
        keys.extend((1..=9).map(|n| (format!("doorkey_{n}"), 1 + n, KEY_SHIFT)));
        for (action, scan_code, modifier) in keys {
            if !self
                .game
                .iter()
                .any(|b| b.action.eq_ignore_ascii_case(&action))
            {
                self.game.push(KeyBinding {
                    action,
                    scan_code,
                    modifier,
                });
            }
        }
        for (action, scan) in [
            ("view_interiorcam_minus", 203),
            ("view_interiorcam_plus", 205),
        ] {
            let other = self.game.iter().any(|b| {
                b.action.eq_ignore_ascii_case(action)
                    && !(b.scan_code == scan && b.modifier == KEY_CTRL)
            });
            if other {
                self.game.retain(|b| {
                    !(b.action.eq_ignore_ascii_case(action)
                        && b.scan_code == scan
                        && b.modifier == KEY_CTRL)
                });
            } else {
                self.game.retain(|b| !b.action.eq_ignore_ascii_case(action));
                self.game.push(KeyBinding {
                    action: action.into(),
                    scan_code: scan,
                    modifier: 0,
                });
            }
        }
        // Second bindings of the same action, listed like any other entry (F12 and Page Down as extra keys): added
        // while the action still sits on its first key
        for (action, first, extra) in [
            ("screenshot", (25, KEY_SHIFT | KEY_CTRL), (88, 0)),
            ("tutorial_next", (28, 0), (209, 0)),
        ] {
            let has = |sc: i32, m: i32| {
                self.game.iter().any(|b| {
                    b.action.eq_ignore_ascii_case(action) && b.scan_code == sc && b.modifier == m
                })
            };
            if has(first.0, first.1) && !has(extra.0, extra.1) {
                self.game.push(KeyBinding {
                    action: action.into(),
                    scan_code: extra.0,
                    modifier: extra.1,
                });
            }
        }
        self
    }

    pub fn with_vr_defaults(mut self) -> Self {
        // VR controls are included in the same editable list as the game's
        // other keys. An existing entry (including an unbound one) wins.
        for (action, scan_code, modifier) in [
            ("vr_recenter", 19, KEY_SHIFT | KEY_CTRL),
            ("vr_toggle_desktop_mirror", 65, 0),
            ("vr_toggle_mode", 66, 0),
            ("vr_toggle_navigator", 49, KEY_SHIFT | KEY_CTRL),
            ("vr_position_navigator", 50, KEY_SHIFT | KEY_CTRL),
            ("vr_nav_confirm", 28, 0),
            ("vr_nav_cancel", 1, 0),
            ("vr_nav_reset", 19, 0),
        ] {
            if !self
                .game
                .iter()
                .any(|b| b.action.eq_ignore_ascii_case(action))
            {
                self.game.push(KeyBinding {
                    action: action.into(),
                    scan_code,
                    modifier,
                });
            }
        }
        self
    }

    /// Write a `keyboard.cfg` the game (and this same loader) can read back: one
    /// `[game]`/`[vehicles]` section, each entry as `action / scan code / modifier`, blank
    /// lines between entries as the original ships it (some tools that read the file split
    /// on the blank line rather than the keyword).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut t = String::new();
        for (section, list) in [("game", &self.game), ("vehicles", &self.vehicles)] {
            t.push_str(&format!("[{section}]\r\n"));
            for b in list {
                t.push_str(&format!(
                    "\r\n[entry]\r\n{}\r\n{}\r\n{}\r\n",
                    b.action, b.scan_code, b.modifier
                ));
            }
            t.push_str("\r\n");
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, t)
    }
}

/// `.kyb`: `scancode<TAB>name` lines.
pub fn load_key_names(path: &Path) -> Result<Vec<(i32, String)>, ::legacy_config::CfgError> {
    let f = CfgFile::read(path)?;
    Ok(f.lines
        .iter()
        .filter_map(|l| {
            let (a, b) = l.split_once('\t')?;
            Some((::legacy_config::parse_i32(a), b.trim().to_string()))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_toggles_are_unbound_and_preserve_existing_bindings() {
        let custom = KeyBinding {
            action: "BLINKER_LEFT_TOGGLE".into(),
            scan_code: 44,
            modifier: 0,
        };
        let cfg = KeyboardCfg {
            vehicles: vec![custom.clone()],
            ..Default::default()
        }
            .with_game_defaults()
            .with_game_defaults();
        assert_eq!(
            cfg.vehicles
                .iter()
                .filter(|b| b.action.eq_ignore_ascii_case("blinker_left_toggle"))
                .count(),
            1
        );
        assert!(cfg.vehicles.contains(&custom));
        let right = cfg
            .vehicles
            .iter()
            .find(|b| b.action == "blinker_right_toggle")
            .unwrap();
        assert_eq!((right.scan_code, right.modifier), (0, 0));
    }

    #[test]
    fn the_chat_keys_are_in_the_list_and_a_moved_one_stays_moved() {
        let cfg = KeyboardCfg::default().with_game_defaults();
        let key = |c: &KeyboardCfg, a: &str| {
            c.game
                .iter()
                .filter(|b| b.action == a)
                .map(|b| (b.scan_code, b.modifier))
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&cfg, "chat_toggle"), vec![(47, 0)]);
        assert_eq!(key(&cfg, "chat_open"), vec![(53, 0)]);
        let moved = KeyboardCfg {
            game: vec![KeyBinding {
                action: "chat_open".into(),
                scan_code: 20,
                modifier: KEY_CTRL,
            }],
            ..Default::default()
        }
            .with_game_defaults();
        assert_eq!(key(&moved, "chat_open"), vec![(20, KEY_CTRL)]);
    }

    #[test]
    fn the_chat_leaves_the_manual_gearbox_its_gear_down_key() {
        // OMSI's stock file: '/' (53) is kw_s_minus, so the chat line opens on the key left of 1
        let gear = KeyBinding {
            action: "kw_s_minus".into(),
            scan_code: 53,
            modifier: 0,
        };
        let key = |c: &KeyboardCfg, a: &str| {
            c.game
                .iter()
                .filter(|b| b.action == a)
                .map(|b| (b.scan_code, b.modifier))
                .collect::<Vec<_>>()
        };
        let cfg = KeyboardCfg {
            vehicles: vec![gear.clone()],
            ..Default::default()
        }
            .with_game_defaults();
        assert_eq!(key(&cfg, "chat_open"), vec![(41, 0)]);
        assert!(cfg.vehicles.contains(&gear));
        // a file saved while the chat was on '/' as well: moved off it, once
        let old = KeyboardCfg {
            game: vec![KeyBinding {
                action: "chat_open".into(),
                scan_code: 53,
                modifier: 0,
            }],
            vehicles: vec![gear.clone()],
        }
            .with_game_defaults()
            .with_game_defaults();
        assert_eq!(key(&old, "chat_open"), vec![(41, 0)]);
        // '/' moved to Shift+'/' by the player stays there
        let shifted = KeyboardCfg {
            game: vec![KeyBinding {
                action: "chat_open".into(),
                scan_code: 53,
                modifier: KEY_SHIFT,
            }],
            vehicles: vec![gear],
        }
            .with_game_defaults();
        assert_eq!(key(&shifted, "chat_open"), vec![(53, KEY_SHIFT)]);
    }

    #[test]
    fn the_third_value_is_held_shift_ctrl_as_omsi_reads_it() {
        let b = |m: i32| KeyBinding {
            action: "a".into(),
            scan_code: 38,
            modifier: m,
        };
        // throttle-like (1: held): the plain key, not Shift+key
        assert!(b(1).matches(0) && !b(1).matches(KEY_SHIFT));
        // Standlicht: Shift+L; exit: Ctrl+Q; the changer: Shift+Ctrl
        assert!(
            b(2).matches(chord(true, false, false)) && !b(2).matches(chord(false, true, false))
        );
        assert!(
            b(4).matches(chord(false, true, false)) && !b(4).matches(chord(true, false, false))
        );
        assert!(
            b(6).matches(chord(true, true, false))
                && b(3).matches(KEY_SHIFT)
                && b(5).matches(KEY_CTRL)
        );
        // Alt: Omsi.exe's keys fire with it held; ours with Alt want it
        assert!(b(0).matches(KEY_ALT) && b(8).matches(KEY_ALT) && !b(8).matches(0));
        assert_eq!(b(3).chord(), KEY_SHIFT);
    }

    #[test]
    fn vr_defaults_keep_custom_and_unbound_keys() {
        let custom = KeyBinding {
            action: "vr_recenter".into(),
            scan_code: 0,
            modifier: 0,
        };
        let cfg = KeyboardCfg {
            game: vec![custom.clone()],
            ..Default::default()
        }
            .with_vr_defaults()
            .with_vr_defaults();
        assert_eq!(
            cfg.game
                .iter()
                .filter(|b| b.action == "vr_recenter")
                .count(),
            1
        );
        assert!(cfg.game.contains(&custom));
        assert!(
            cfg.game
                .iter()
                .any(|b| b.action == "vr_toggle_mode" && b.scan_code == 66)
        );
        assert_eq!(
            cfg.game
                .iter()
                .filter(|b| b.action == "vr_toggle_navigator")
                .count(),
            1
        );
        assert!(cfg.game.iter().any(|b| b.action == "vr_toggle_navigator"
            && b.scan_code == 49
            && b.modifier == (KEY_SHIFT | KEY_CTRL)));
    }

    #[test]
    fn keyboard_cfg_round_trips_through_save() {
        let k = KeyboardCfg {
            game: vec![KeyBinding {
                action: "exit".into(),
                scan_code: 1,
                modifier: 4,
            }],
            vehicles: vec![
                KeyBinding {
                    action: "kw_blinker_links".into(),
                    scan_code: 44,
                    modifier: 0,
                },
                KeyBinding {
                    action: "kw_m_enginestart".into(),
                    scan_code: 50,
                    modifier: 1,
                },
            ],
        };
        let path =
            std::env::temp_dir().join(format!("omsi-keyboard-cfg-test-{}.cfg", std::process::id()));
        k.save(&path).unwrap();
        let back = KeyboardCfg::load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back, k);
    }
}
