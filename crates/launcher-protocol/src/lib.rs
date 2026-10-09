pub mod link;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Write};

pub const VERSION: &str = "1";
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Message {
    pub fn new(kind: &str, payload: Value) -> Message {
        Message {
            kind: kind.into(),
            payload,
            ..Default::default()
        }
    }

    pub fn reply(&self, result: Result<Value, String>) -> Message {
        let (payload, error) = match result {
            Ok(v) => (v, None),
            Err(e) => (Value::Null, Some(e)),
        };
        Message {
            kind: self.kind.clone(),
            request_id: self.request_id.clone(),
            payload,
            error,
        }
    }
}

pub fn encode(m: &Message) -> io::Result<Vec<u8>> {
    let data = serde_json::to_vec(m)?;
    if data.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a frame of {} bytes is over the limit of {MAX_FRAME}", data.len()),
        ));
    }
    let mut frame = Vec::with_capacity(4 + data.len());
    frame.extend_from_slice(&(data.len() as u32).to_be_bytes());
    frame.extend_from_slice(&data);
    Ok(frame)
}

pub fn write_frame(w: &mut impl Write, m: &Message) -> io::Result<()> {
    w.write_all(&encode(m)?)?;
    w.flush()
}

/// `Ok(None)`: the other side closed the stream.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Message>> {
    read_frame_max(r, MAX_FRAME)
}

pub fn read_frame_max(r: &mut impl Read, max: usize) -> io::Result<Option<Message>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a frame of {len} bytes is over the limit of {max}"),
        ));
    }
    let mut data = vec![0u8; len];
    r.read_exact(&mut data)?;
    serde_json::from_slice(&data)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frames_round_trip_and_split_anywhere() {
        let a = Message {
            kind: "maps".into(),
            request_id: Some("req_1".into()),
            payload: json!({"filter": "Spandau"}),
            error: None,
        };
        let b = a.reply(Err("no OMSI 2 folder".into()));
        let mut bytes = encode(&a).unwrap();
        bytes.extend(encode(&b).unwrap());
        let mut r = io::Cursor::new(bytes);
        assert_eq!(read_frame(&mut r).unwrap(), Some(a));
        let back = read_frame(&mut r).unwrap().unwrap();
        assert_eq!(back.error.as_deref(), Some("no OMSI 2 folder"));
        assert_eq!(back.request_id.as_deref(), Some("req_1"));
        assert_eq!(read_frame(&mut r).unwrap(), None);
    }

    #[test]
    fn the_launchers_field_names_are_kept() {
        let text = String::from_utf8(encode(&Message::new("instances_changed", json!([]))).unwrap()[4..].to_vec()).unwrap();
        assert_eq!(text, r#"{"type":"instances_changed","payload":[]}"#);
        let m: Message = serde_json::from_str(r#"{"type":"handshake","requestId":"r","payload":{}}"#).unwrap();
        assert_eq!(m.request_id.as_deref(), Some("r"));
    }

    #[test]
    fn oversized_and_broken_frames_are_refused() {
        let mut r = io::Cursor::new(((MAX_FRAME + 1) as u32).to_be_bytes().to_vec());
        assert!(read_frame(&mut r).is_err());
        let mut r = io::Cursor::new([&3u32.to_be_bytes()[..], b"{x}"].concat());
        assert!(read_frame(&mut r).is_err());
        let mut r = io::Cursor::new([&9u32.to_be_bytes()[..], b"{\"a\":1}"].concat());
        assert!(read_frame(&mut r).is_err(), "cut short");
    }
}
