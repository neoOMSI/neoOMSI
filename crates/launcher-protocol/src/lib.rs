pub mod api;
pub mod link;

use prost::Message;
use std::io::{self, Read, Write};

pub const MAX_FRAME: usize = 16 * 1024 * 1024;

pub fn frame(data: &[u8]) -> io::Result<Vec<u8>> {
    if data.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a frame of {} bytes is over the limit of {MAX_FRAME}", data.len()),
        ));
    }
    let mut frame = Vec::with_capacity(4 + data.len());
    frame.extend_from_slice(&(data.len() as u32).to_be_bytes());
    frame.extend_from_slice(data);
    Ok(frame)
}

pub fn encode(m: &impl Message) -> io::Result<Vec<u8>> {
    frame(&m.encode_to_vec())
}

pub fn write_frame(w: &mut impl Write, m: &impl Message) -> io::Result<()> {
    w.write_all(&encode(m)?)?;
    w.flush()
}

/// `Ok(None)`: the other side closed the stream.
pub fn read_frame<M: Message + Default>(r: &mut impl Read) -> io::Result<Option<M>> {
    read_frame_max(r, MAX_FRAME)
}

pub fn read_frame_max<M: Message + Default>(
    r: &mut impl Read,
    max: usize,
) -> io::Result<Option<M>> {
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
    M::decode(data.as_slice())
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::{Frame, LinesArgs, Request, frame, request};

    #[test]
    fn frames_round_trip_and_split_anywhere() {
        let a = Frame {
            request_id: "req_1".into(),
            body: Some(frame::Body::Request(Request {
                command: Some(request::Command::Lines(LinesArgs {
                    map: "maps/Spandau/global.cfg".into(),
                    date: String::new(),
                })),
            })),
            ..Default::default()
        };
        let b = Frame {
            request_id: "req_1".into(),
            error: "no OMSI 2 folder".into(),
            body: None,
        };
        let mut bytes = encode(&a).unwrap();
        bytes.extend(encode(&b).unwrap());
        let mut r = io::Cursor::new(bytes);
        assert_eq!(read_frame(&mut r).unwrap(), Some(a));
        assert_eq!(read_frame(&mut r).unwrap(), Some(b));
        assert_eq!(read_frame::<Frame>(&mut r).unwrap(), None);
    }

    #[derive(Clone, PartialEq, Message)]
    struct ControllerBeforeAxisMode {
        #[prost(string, tag = "1")]
        name: String,
        #[prost(message, repeated, tag = "6")]
        axes: Vec<api::ControllerAxis>,
        #[prost(bool, optional, tag = "8")]
        ff_invert: Option<bool>,
    }

    #[test]
    fn controller_axis_mode_is_optional_and_old_decoders_keep_existing_fields() {
        for mode in [None, Some("native"), Some(""), Some("future_mode")] {
            let controller = api::Controller {
                name: "T128".into(),
                axes: vec![api::ControllerAxis {
                    reversed: true,
                    calibration: Some(api::AxisCalibration {
                        min: -0.8,
                        max: 0.9,
                        centre: Some(0.01),
                        deadzone: None,
                    }),
                    ..Default::default()
                }],
                ff_invert: Some(true),
                axis_mode: mode.map(str::to_owned),
                ..Default::default()
            };
            let bytes = controller.encode_to_vec();
            assert_eq!(
                api::Controller::decode(bytes.as_slice()).unwrap(),
                controller
            );
            let legacy = ControllerBeforeAxisMode::decode(bytes.as_slice()).unwrap();
            assert_eq!(legacy.name, controller.name);
            assert_eq!(legacy.axes, controller.axes);
            assert_eq!(legacy.ff_invert, controller.ff_invert);
            let legacy_bytes = legacy.encode_to_vec();
            let restored = api::Controller::decode(legacy_bytes.as_slice()).unwrap();
            assert_eq!(restored.axis_mode, None);
            assert_eq!(restored.axes, controller.axes);
            assert_eq!(restored.ff_invert, Some(true));
        }
    }

    #[test]
    fn oversized_broken_and_json_frames_are_refused() {
        let mut r = io::Cursor::new(((MAX_FRAME + 1) as u32).to_be_bytes().to_vec());
        assert!(read_frame::<Frame>(&mut r).is_err());
        let mut r = io::Cursor::new([&1u32.to_be_bytes()[..], &[0xff]].concat());
        assert!(read_frame::<Frame>(&mut r).is_err());
        let mut r = io::Cursor::new([&9u32.to_be_bytes()[..], &[0x0a, 0x07, b'r']].concat());
        assert!(read_frame::<Frame>(&mut r).is_err(), "cut short");
        let json = super::frame(br#"{"type":"handshake","payload":{}}"#).unwrap();
        assert!(read_frame::<Frame>(&mut io::Cursor::new(json)).is_err());
    }
}
