pub const VOICE_MAGIC: u8 = 0xB5;
pub const VOICE_HEADER: usize = 6;
pub const MAX_VOICE_PAYLOAD: usize = 400;
pub const FRAME_MS: u32 = 20;
pub const FLAG_RADIO: u8 = 1;
const KNOWN_FLAGS: u8 = FLAG_RADIO;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceFrame {
    pub id: u32,
    pub seq: u16,
    pub flags: u8,
    pub data: Vec<u8>,
}

impl VoiceFrame {
    pub fn radio(&self) -> bool {
        self.flags & FLAG_RADIO != 0
    }
}

pub fn encode_voice(id: u32, seq: u16, flags: u8, payload: &[u8]) -> Vec<u8> {
    let id = id.min(u16::MAX as u32) as u16;
    let mut d = Vec::with_capacity(VOICE_HEADER + payload.len());
    d.push(VOICE_MAGIC);
    d.extend_from_slice(&id.to_le_bytes());
    d.extend_from_slice(&seq.to_le_bytes());
    d.push(flags & KNOWN_FLAGS);
    d.extend_from_slice(&payload[..payload.len().min(MAX_VOICE_PAYLOAD)]);
    d
}

pub fn decode_voice(data: &[u8]) -> Option<VoiceFrame> {
    if data.len() <= VOICE_HEADER
        || data.len() > VOICE_HEADER + MAX_VOICE_PAYLOAD
        || data[0] != VOICE_MAGIC
    {
        return None;
    }
    let id = u16::from_le_bytes([data[1], data[2]]) as u32;
    if id == 0 {
        return None;
    }
    Some(VoiceFrame {
        id,
        seq: u16::from_le_bytes([data[3], data[4]]),
        flags: data[5] & KNOWN_FLAGS,
        data: data[VOICE_HEADER..].to_vec(),
    })
}

/// What a host allows: voice at all, and how far a voice carries (m).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceConfig {
    pub enabled: bool,
    pub range: f32,
}

pub const DEFAULT_RANGE: f32 = 60.0;
pub const MIN_RANGE: f32 = 5.0;
pub const MAX_RANGE: f32 = 500.0;

impl Default for VoiceConfig {
    fn default() -> Self {
        VoiceConfig {
            enabled: true,
            range: DEFAULT_RANGE,
        }
    }
}

impl VoiceConfig {
    pub const OFF: VoiceConfig = VoiceConfig {
        enabled: false,
        range: DEFAULT_RANGE,
    };

    /// As a message field: the range in metres, `0` for no voice.
    pub fn field(&self) -> String {
        if self.enabled {
            format!("{:.0}", self.range.clamp(MIN_RANGE, MAX_RANGE))
        } else {
            "0".into()
        }
    }

    /// An empty field (a host without voice) is no voice.
    pub fn from_field(s: &str) -> VoiceConfig {
        match s.trim().parse::<f32>() {
            Ok(r) if r.is_finite() && r > 0.0 => VoiceConfig {
                enabled: true,
                range: r.clamp(MIN_RANGE, MAX_RANGE),
            },
            _ => VoiceConfig::OFF,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let d = encode_voice(7, 65535, FLAG_RADIO, &[1, 2, 3]);
        assert_eq!(d.len(), VOICE_HEADER + 3);
        let f = decode_voice(&d).unwrap();
        assert_eq!(
            (f.id, f.seq, f.radio(), f.data.as_slice()),
            (7, 65535, true, &[1u8, 2, 3][..])
        );
    }

    #[test]
    fn garbage_is_not_a_frame() {
        assert!(
            decode_voice(&[VOICE_MAGIC, 1, 0, 0, 0, 0]).is_none(),
            "no payload"
        );
        assert!(
            decode_voice(&encode_voice(0, 1, 0, &[1])).is_none(),
            "nobody's"
        );
        let mut long = encode_voice(3, 1, 0, &[0; MAX_VOICE_PAYLOAD]);
        assert!(decode_voice(&long).is_some());
        long.push(0);
        assert!(decode_voice(&long).is_none());
        assert_eq!(
            decode_voice(&encode_voice(3, 1, 0xFE, &[1])).unwrap().flags,
            0
        );
    }

    #[test]
    fn config_fields() {
        let on = VoiceConfig {
            enabled: true,
            range: 80.0,
        };
        assert_eq!(VoiceConfig::from_field(&on.field()), on);
        assert!(!VoiceConfig::from_field("0").enabled);
        assert!(!VoiceConfig::from_field("").enabled);
        assert!(!VoiceConfig::from_field("NaN").enabled);
        assert_eq!(VoiceConfig::from_field("99999").range, MAX_RANGE);
    }
}
