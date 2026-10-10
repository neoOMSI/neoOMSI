use serde::Deserialize;
use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/neoomsi.launcher.rs"));
include!(concat!(env!("OUT_DIR"), "/commands.rs"));

pub const VERSION: &str = "1";

#[derive(Deserialize, Default)]
#[serde(default)]
struct LanFile {
    role: String,
    name: String,
    code: Option<String>,
    address: Option<String>,
    addresses: Vec<LanFileAddress>,
    trying: Option<Vec<String>>,
    port: Option<u32>,
    session: String,
    tunnel: Option<String>,
    connected: bool,
    rejected: Option<String>,
    warnings: Vec<String>,
    host_name: Option<String>,
    map: String,
    players: Vec<LanFilePlayer>,
    chat: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LanFileAddress {
    label: String,
    address: String,
    kind: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LanFilePlayer {
    id: u32,
    name: String,
    bus: String,
    line: String,
    destination: String,
    passengers: u32,
    #[serde(rename = "where")]
    location: Option<String>,
    drawn: bool,
}

/// A field the game leaves out reads as empty; a file that does not parse, as an empty status.
pub fn lan_status(v: &Value) -> LanStatus {
    let f = LanFile::deserialize(v).unwrap_or_default();
    LanStatus {
        role: match f.role.as_str() {
            "host" => LanRole::Host,
            "client" => LanRole::Client,
            _ => LanRole::Unspecified,
        }
        .into(),
        name: f.name,
        code: f.code,
        address: f.address,
        addresses: f
            .addresses
            .into_iter()
            .map(|a| LanAddress {
                label: a.label,
                address: a.address,
                kind: a.kind,
            })
            .collect(),
        trying: f.trying.unwrap_or_default(),
        port: f.port,
        session: f.session,
        tunnel: f.tunnel,
        connected: f.connected,
        rejected: f.rejected,
        warnings: f.warnings,
        host_name: f.host_name,
        map: f.map,
        players: f
            .players
            .into_iter()
            .map(|p| LanPlayer {
                id: p.id,
                name: p.name,
                bus: p.bus,
                line: p.line,
                destination: p.destination,
                passengers: p.passengers,
                location: p.location,
                drawn: p.drawn,
            })
            .collect(),
        chat: f.chat,
    }
}

trait Setting: Sized {
    fn read(v: &Value) -> Option<Self>;
    fn json(&self) -> Value;
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.trim().parse().ok())
        .filter(|x: &f64| x.is_finite())
}

impl Setting for bool {
    fn read(v: &Value) -> Option<bool> {
        v.as_bool()
    }
    fn json(&self) -> Value {
        Value::from(*self)
    }
}

impl Setting for f64 {
    fn read(v: &Value) -> Option<f64> {
        number(v)
    }
    fn json(&self) -> Value {
        Value::from(*self)
    }
}

impl Setting for i32 {
    fn read(v: &Value) -> Option<i32> {
        number(v).map(|x| x.round() as i32)
    }
    fn json(&self) -> Value {
        Value::from(*self)
    }
}

impl Setting for u32 {
    fn read(v: &Value) -> Option<u32> {
        number(v).map(|x| x.round() as u32)
    }
    fn json(&self) -> Value {
        Value::from(*self)
    }
}

impl Setting for String {
    fn read(v: &Value) -> Option<String> {
        v.as_str().map(str::to_string)
    }
    fn json(&self) -> Value {
        Value::from(self.as_str())
    }
}

impl Setting for AutoNumber {
    fn read(v: &Value) -> Option<AutoNumber> {
        if v.as_str() == Some("auto") {
            return Some(AutoNumber {
                automatic: true,
                value: 0.0,
            });
        }
        number(v).map(|value| AutoNumber {
            automatic: false,
            value,
        })
    }
    fn json(&self) -> Value {
        if self.automatic { Value::from("auto") } else { Value::from(self.value) }
    }
}

/// The settings travel as the engine's settings table keeps them: the field names are its keys,
/// and a choice is its value's name in lower case.
macro_rules! settings_fields {
    (plain [$($key:ident),*] choices [$($choice:ident: $ty:ident = $prefix:literal),*]) => {
        pub const SETTING_FIELDS: &[&str] = &[$(stringify!($key),)* $(stringify!($choice)),*];

        pub fn settings(v: &Value) -> Settings {
            Settings {
                $($key: v.get(stringify!($key)).and_then(Setting::read),)*
                $($choice: v
                    .get(stringify!($choice))
                    .and_then(Value::as_str)
                    .and_then(|t| $ty::from_str_name(&format!("{}{}", $prefix, t.to_ascii_uppercase())))
                    .map(Into::into),)*
            }
        }

        pub fn settings_json(s: &Settings) -> Value {
            let mut m = serde_json::Map::new();
            $(if let Some(x) = &s.$key {
                m.insert(stringify!($key).into(), x.json());
            })*
            $(if let Some(x) = s.$choice.and_then(|x| $ty::try_from(x).ok()).filter(|x| *x != $ty::Unspecified) {
                m.insert(stringify!($choice).into(), x.as_str_name()[$prefix.len()..].to_ascii_lowercase().into());
            })*
            Value::Object(m)
        }
    };
}

include!(concat!(env!("OUT_DIR"), "/settings.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_handshake_and_shutdown_are_not_listed_as_commands() {
        assert!(!COMMANDS.contains(&"handshake") && !COMMANDS.contains(&"shutdown"));
        assert!(COMMANDS.contains(&"launch") && COMMANDS.contains(&"version"));
    }

    #[test]
    fn settings_keep_their_kinds() {
        let page = json!({
            "vsync": true,
            "msaa": 4,
            "ui_opacity": 0.8,
            "language": "de",
            "time_speed": "2",
            "view_distance": "auto",
            "max_obj_dist": "900",
            "window_mode": "borderless",
            "graphics": "vanilla_plus",
            "graphics_api": "dx12",
            "enhanced": false,
            "units": "parsecs",
            "head_pitch": -12.0,
        });
        let s = settings(&page);
        assert_eq!((s.vsync, s.msaa, s.time_speed), (Some(true), Some(4), Some(2.0)));
        assert_eq!(s.head_pitch, Some(-12.0));
        assert_eq!(s.window_mode(), WindowMode::Borderless);
        assert_eq!(s.graphics(), GraphicsMode::VanillaPlus);
        assert_eq!(s.units, None, "not one of the choices");
        assert_eq!(
            settings_json(&s),
            json!({
                "vsync": true,
                "msaa": 4,
                "ui_opacity": 0.8,
                "language": "de",
                "time_speed": 2.0,
                "view_distance": "auto",
                "max_obj_dist": 900.0,
                "window_mode": "borderless",
                "graphics": "vanilla_plus",
                "graphics_api": "dx12",
                "head_pitch": -12.0,
            })
        );
    }

    #[test]
    fn only_the_settings_that_are_set_are_saved() {
        let changes = Settings {
            pax_models: Some(PaxModels::Realistic.into()),
            ..Default::default()
        };
        assert_eq!(settings_json(&changes), json!({ "pax_models": "realistic" }));
    }

    #[test]
    fn the_lan_file_is_read_as_the_game_writes_it() {
        let s = lan_status(&json!({
            "role": "client",
            "code": null,
            "trying": ["10.0.0.2:27015"],
            "port": 27016,
            "connected": false,
            "rejected": "the session is full",
            "players": [{ "id": 3, "name": "Lena", "where": "120 m ahead", "drawn": true }],
            "updated": 1760000000,
        }));
        assert_eq!(s.role(), LanRole::Client);
        assert_eq!((s.code.as_deref(), s.port), (None, Some(27016)));
        assert_eq!(s.rejected.as_deref(), Some("the session is full"));
        assert_eq!(s.players[0].location.as_deref(), Some("120 m ahead"));
        assert_eq!(lan_status(&json!({ "role": 1 })), LanStatus::default());
    }
}
