//! Head tracking: the head's pose from opentrack's "UDP over network" output (six little-
//! endian doubles - x, y, z in cm, yaw, pitch, roll in degrees - to UDP port 4242). opentrack
//! takes TrackIR, Tobii, webcams (neuralnet tracker) and phones, on every platform.
//!
//! On Windows the pose is also read as Omsi.exe reads it: from TrackIR's own software
//! through NaturalPoint's NPClient (the DLL the TrackIR software registers, opentrack's
//! stand-in for it alike), and from opentrack's default output there, "freetrack 2.0
//! Enhanced" (the `FT_SharedMem` mapping). A UDP pose wins over both, freetrack over TrackIR
//! (opentrack's NPClient hands on the freetrack poses, but not in TrackIR's axes).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The last pose received: x, y, z (cm; right, up, back) and yaw, pitch, roll (degrees).
#[derive(Debug, Clone, Copy, Default)]
pub struct HeadPose {
    pub pos: [f32; 3],
    pub rot: [f32; 3],
}

pub struct HeadTracker {
    last: Arc<Mutex<Option<(HeadPose, Instant)>>>,
}

impl HeadTracker {
    /// Listen on `port` (on every interface: opentrack may run on another machine).
    /// `window` is the game window's handle, which TrackIR registers the game by (Windows).
    pub fn start(port: u16, window: Option<isize>) -> Option<HeadTracker> {
        // (the port may be taken: opentrack's own "UDP over network" input listens on 4242
        // too, which is how FreePIE hands it a TrackIR's pose. On Windows the freetrack
        // mapping is still read then; elsewhere there is nothing to read.)
        let sock = match std::net::UdpSocket::bind(("0.0.0.0", port)) {
            Ok(s) => Some(s),
            Err(e) if cfg!(windows) => {
                log::warn!(
                    "head tracking: cannot listen on UDP port {port} ({e}), reading TrackIR and opentrack's freetrack output only"
                );
                None
            }
            Err(e) => {
                log::warn!("head tracking: cannot listen on UDP port {port}: {e}");
                return None;
            }
        };
        // (on Windows the loop also polls TrackIR and the freetrack mapping between datagrams)
        let wait = if cfg!(windows) { 10 } else { 500 };
        #[cfg(not(windows))]
        let _ = window;
        if let Some(sock) = &sock {
            let _ = sock.set_read_timeout(Some(Duration::from_millis(wait)));
        }
        let listening = sock.is_some();
        let last: Arc<Mutex<Option<(HeadPose, Instant)>>> = Arc::default();
        let out = last.clone();
        std::thread::Builder::new()
            .name("head tracking".into())
            .spawn(move || {
                let mut buf = [0u8; 64];
                let mut announced = false;
                #[cfg(windows)]
                let mut trackir = window.map(npclient::Reader::new);
                #[cfg(windows)]
                let mut freetrack = freetrack::Reader::default();
                #[cfg(windows)]
                let mut last_udp: Option<Instant> = None;
                #[cfg(windows)]
                let mut last_freetrack: Option<Instant> = None;
                loop {
                    // (the game's end takes the thread with it)
                    if Arc::strong_count(&out) == 1 {
                        return;
                    }
                    let got = match &sock {
                        Some(sock) => sock.recv(&mut buf),
                        None => {
                            std::thread::sleep(Duration::from_millis(wait));
                            Err(std::io::ErrorKind::WouldBlock.into())
                        }
                    };
                    let Ok(n) = got else {
                        #[cfg(windows)]
                        if last_udp.is_none_or(|t| t.elapsed() >= Duration::from_millis(500)) {
                            let pose = match freetrack.poll() {
                                Some(pose) => {
                                    last_freetrack = Some(Instant::now());
                                    Some(pose)
                                }
                                // (while opentrack writes the mapping, its NPClient only
                                // hands on the same poses: they come from the mapping)
                                None if last_freetrack
                                    .is_some_and(|t| t.elapsed() < Duration::from_millis(500)) =>
                                {
                                    None
                                }
                                None => trackir.as_mut().and_then(npclient::Reader::poll),
                            };
                            if let Some(pose) = pose {
                                *out.lock().unwrap() = Some((pose, Instant::now()));
                            }
                        }
                        continue;
                    };
                    if n < 48 {
                        continue;
                    }
                    let d = |i: usize| {
                        f64::from_le_bytes(buf[i * 8..i * 8 + 8].try_into().unwrap()) as f32
                    };
                    let v = [d(0), d(1), d(2), d(3), d(4), d(5)];
                    if v.iter().any(|x| !x.is_finite()) {
                        continue;
                    }
                    if !announced {
                        announced = true;
                        log::info!("head tracking: receiving poses on UDP port {port}");
                    }
                    #[cfg(windows)]
                    {
                        last_udp = Some(Instant::now());
                    }
                    *out.lock().unwrap() = Some((
                        HeadPose {
                            pos: [v[0], v[1], v[2]],
                            rot: [v[3], v[4], v[5]],
                        },
                        Instant::now(),
                    ));
                }
            })
            .ok()?;
        if listening {
            log::info!("head tracking: listening for opentrack on UDP port {port}");
        }
        Some(HeadTracker { last })
    }

    /// The pose, while poses keep coming (none for half a second: the head is centred).
    pub fn pose(&self) -> Option<HeadPose> {
        let l = self.last.lock().ok()?;
        l.filter(|(_, t)| t.elapsed() < Duration::from_millis(500))
            .map(|(p, _)| p)
    }
}

/// A freetrack pose as opentrack writes it into `FT_SharedMem` - yaw, pitch, roll in
/// radians with yaw and pitch negated, x, y, z in mm - in the UDP output's terms (cm and
/// degrees).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn freetrack_pose(yaw: f32, pitch: f32, roll: f32, x: f32, y: f32, z: f32) -> HeadPose {
    HeadPose {
        pos: [x / 10.0, y / 10.0, z / 10.0],
        rot: [-yaw.to_degrees(), -pitch.to_degrees(), roll.to_degrees()],
    }
}

/// A TrackIR pose as NPClient hands it out (`TRACKIRDATA`'s roll, pitch, yaw, x, y, z,
/// +-16383 for +-180 degrees and +-50 cm), in the UDP output's terms. None for what no
/// tracker sends (encrypted data reads as such).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn npclient_pose(roll: f32, pitch: f32, yaw: f32, x: f32, y: f32, z: f32) -> Option<HeadPose> {
    const MAX: f32 = 16383.0;
    if [roll, pitch, yaw, x, y, z]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > MAX * 1.01)
    {
        return None;
    }
    let (rad, mm) = (std::f32::consts::PI / MAX, 500.0 / MAX);
    // (TrackIR's sideways lean and roll run the other way to the freetrack mapping's, as
    // driving with a TrackIR showed; opentrack's NPClient does not turn them round)
    Some(freetrack_pose(
        yaw * rad,
        pitch * rad,
        -roll * rad,
        -x * mm,
        y * mm,
        z * mm,
    ))
}

#[cfg(windows)]
mod npclient {
    use super::{HeadPose, npclient_pose};
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::{HSTRING, PCSTR, s, w};

    #[cfg(target_pointer_width = "64")]
    const DLL: &str = "NPClient64.dll";
    #[cfg(not(target_pointer_width = "64"))]
    const DLL: &str = "NPClient.dll";

    /// OMSI's id in TrackIR's game list, which Omsi.exe registers (its poses come unencrypted).
    const PROFILE_ID: u16 = 2225;
    /// `NP_RequestData`: pitch, yaw, roll, x, y, z.
    const REQUEST: u16 = 1 | 2 | 4 | 16 | 32 | 64;

    /// `TRACKIRDATA`.
    #[repr(C)]
    #[derive(Default)]
    struct Data {
        status: u16,
        frame: u16,
        io: u32,
        roll: f32,
        pitch: f32,
        yaw: f32,
        x: f32,
        y: f32,
        z: f32,
        raw_delta_smooth: [f32; 9],
    }

    type Call = unsafe extern "system" fn() -> i32;
    type CallWith = unsafe extern "system" fn(u16) -> i32;

    struct Client {
        lib: HMODULE,
        get_data: unsafe extern "system" fn(*mut Data) -> i32,
        stop: Call,
        unregister: Call,
        start_cursor: Option<Call>,
    }

    /// The folder NPClient is in, as the TrackIR software (or opentrack) registered it.
    fn location() -> Option<String> {
        let mut buf = [0u16; 1024];
        let mut len = std::mem::size_of_val(&buf) as u32;
        // SAFETY: the buffer is `len` bytes long.
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\NaturalPoint\\NATURALPOINT\\NPClient Location"),
                w!("Path"),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        }
        .ok()
        .ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize / 2]);
        let path = path.trim_end_matches('\0');
        (!path.is_empty()).then(|| path.to_owned())
    }

    /// `name` from `lib`, as the function type `F`.
    ///
    /// SAFETY: `F` must be the export's signature.
    unsafe fn export<F: Copy>(lib: HMODULE, name: PCSTR) -> Result<F, String> {
        // SAFETY: a plain lookup; a function pointer is as wide as `F`.
        match unsafe { GetProcAddress(lib, name) } {
            Some(f) => Ok(unsafe { std::mem::transmute_copy(&f) }),
            None => Err(format!("{} is missing", unsafe { name.display() })),
        }
    }

    impl Client {
        /// Load NPClient and register the game with it. Ok(None): nothing registered NPClient.
        fn open(window: isize) -> Result<Option<Client>, String> {
            let Some(dir) = location() else {
                return Ok(None);
            };
            let path = std::path::Path::new(&dir).join(DLL);
            // SAFETY: the DLL is the one the TrackIR software (or opentrack) installed for
            // its games; loading it is what they are made for.
            let lib = unsafe { LoadLibraryW(&HSTRING::from(path.as_os_str())) }
                .map_err(|e| format!("cannot load {}: {e}", path.display()))?;
            let close = |e: String| {
                // SAFETY: nothing of the library is in use yet.
                let _ = unsafe { FreeLibrary(lib) };
                e
            };
            // SAFETY: the signatures of NPClient.h (NPRESULT is an int; HWND as wide as isize).
            let (register, request, profile, start, stop, unregister, get_data) = unsafe {
                (
                    export::<unsafe extern "system" fn(isize) -> i32>(
                        lib,
                        s!("NP_RegisterWindowHandle"),
                    ),
                    export::<CallWith>(lib, s!("NP_RequestData")),
                    export::<CallWith>(lib, s!("NP_RegisterProgramProfileID")),
                    export::<Call>(lib, s!("NP_StartDataTransmission")),
                    export::<Call>(lib, s!("NP_StopDataTransmission")),
                    export::<Call>(lib, s!("NP_UnregisterWindowHandle")),
                    export(lib, s!("NP_GetData")),
                )
            };
            let (register, request, profile, start) = (
                register.map_err(close)?,
                request.map_err(close)?,
                profile.map_err(close)?,
                start.map_err(close)?,
            );
            let client = Client {
                lib,
                get_data: get_data.map_err(close)?,
                stop: stop.map_err(close)?,
                unregister: unregister.map_err(close)?,
                // SAFETY: as above.
                start_cursor: unsafe { export::<Call>(lib, s!("NP_StartCursor")) }.ok(),
            };
            // SAFETY: the SDK's order; on a failure dropping the client unregisters again.
            unsafe {
                let r = register(window);
                if r != 0 {
                    return Err(format!("NP_RegisterWindowHandle failed ({r})"));
                }
                request(REQUEST);
                let r = profile(PROFILE_ID);
                if r != 0 {
                    return Err(format!("NP_RegisterProgramProfileID failed ({r})"));
                }
                // (the TrackIR mouse emulation keeps off the cursor while the game is on)
                if let Ok(stop_cursor) = export::<Call>(lib, s!("NP_StopCursor")) {
                    stop_cursor();
                }
                let r = start();
                if r != 0 {
                    return Err(format!("NP_StartDataTransmission failed ({r})"));
                }
            }
            log::info!("head tracking: TrackIR through {}", path.display());
            Ok(Some(client))
        }
    }

    impl Drop for Client {
        fn drop(&mut self) {
            // SAFETY: the SDK's way out; nothing calls into the library afterwards.
            unsafe {
                (self.stop)();
                if let Some(start_cursor) = self.start_cursor {
                    start_cursor();
                }
                (self.unregister)();
                let _ = FreeLibrary(self.lib);
            }
        }
    }

    /// TrackIR, registered with once its software is there (tried every few seconds).
    pub struct Reader {
        window: isize,
        client: Option<Client>,
        tried: Option<Instant>,
        failing: Option<Instant>,
        last_frame: Option<u16>,
        last_error: Option<String>,
        announced: bool,
        warned: bool,
    }

    impl Reader {
        pub fn new(window: isize) -> Reader {
            Reader {
                window,
                client: None,
                tried: None,
                failing: None,
                last_frame: None,
                last_error: None,
                announced: false,
                warned: false,
            }
        }

        /// A new pose, when TrackIR has tracked a frame since the last call.
        pub fn poll(&mut self) -> Option<HeadPose> {
            if self.client.is_none() {
                if self
                    .tried
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(5))
                {
                    return None;
                }
                self.tried = Some(Instant::now());
                match Client::open(self.window) {
                    Ok(c) => self.client = c,
                    Err(e) => {
                        if self.last_error.as_ref() != Some(&e) {
                            log::warn!("head tracking: {e}");
                            self.last_error = Some(e);
                        }
                        return None;
                    }
                }
            }
            let client = self.client.as_ref()?;
            let mut d = Data::default();
            // SAFETY: `d` is the TRACKIRDATA NP_GetData fills.
            if unsafe { (client.get_data)(&mut d) } != 0 {
                // (the TrackIR software went away: register anew once it is back)
                if self.failing.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(2)
                {
                    self.client = None;
                    self.failing = None;
                }
                return None;
            }
            self.failing = None;
            // (status 1: tracking paused by the user)
            if d.status != 0 || self.last_frame.replace(d.frame) == Some(d.frame) {
                return None;
            }
            let Some(pose) = npclient_pose(d.roll, d.pitch, d.yaw, d.x, d.y, d.z) else {
                if !self.warned {
                    self.warned = true;
                    log::warn!("head tracking: TrackIR sends poses out of range (encrypted?)");
                }
                return None;
            };
            if !self.announced {
                self.announced = true;
                log::info!("head tracking: receiving poses from TrackIR");
            }
            Some(pose)
        }
    }
}

#[cfg(windows)]
mod freetrack {
    use super::{HeadPose, freetrack_pose};
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Memory::{FILE_MAP_READ, MapViewOfFile, OpenFileMappingW};
    use windows::core::w;

    /// `FTData`: DataID, CamWidth, CamHeight (i32), Yaw, Pitch, Roll, X, Y, Z (f32), ...
    const FIELDS: usize = 9;

    /// The mapping, opened once opentrack has made it (looked for once a second).
    #[derive(Default)]
    pub struct Reader {
        view: Option<(HANDLE, *const u32)>,
        tried: Option<Instant>,
        last_id: Option<u32>,
        announced: bool,
    }

    // (the view is only read, from the head tracking thread)
    unsafe impl Send for Reader {}

    impl Reader {
        /// A new pose, when opentrack has written one since the last call.
        pub fn poll(&mut self) -> Option<HeadPose> {
            if self.view.is_none() {
                if self
                    .tried
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
                {
                    return None;
                }
                self.tried = Some(Instant::now());
                // SAFETY: plain Win32 calls; the view stays mapped for the life of the game.
                unsafe {
                    let handle =
                        OpenFileMappingW(FILE_MAP_READ.0, false, w!("FT_SharedMem")).ok()?;
                    let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, FIELDS * 4);
                    if view.Value.is_null() {
                        let _ = windows::Win32::Foundation::CloseHandle(handle);
                        return None;
                    }
                    self.view = Some((handle, view.Value as *const u32));
                }
            }
            let (_, base) = self.view?;
            // SAFETY: the view spans the FIELDS words read here; opentrack writes them
            // from its own process, hence the volatile reads.
            let word = |i: usize| unsafe { base.add(i).read_volatile() };
            let id = word(0);
            if self.last_id.replace(id) == Some(id) {
                return None;
            }
            let f = |i: usize| f32::from_bits(word(i));
            let v = [f(3), f(4), f(5), f(6), f(7), f(8)];
            if v.iter().any(|x| !x.is_finite()) {
                return None;
            }
            if !self.announced {
                self.announced = true;
                log::info!("head tracking: receiving poses from freetrack (FT_SharedMem)");
            }
            Some(freetrack_pose(v[0], v[1], v[2], v[3], v[4], v[5]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freetrack_pose_is_in_cm_and_degrees() {
        let p = freetrack_pose(
            -0.5f32.to_radians() * 60.0,
            10f32.to_radians(),
            5f32.to_radians(),
            15.0,
            -20.0,
            100.0,
        );
        assert!(
            (p.rot[0] - 30.0).abs() < 1e-3
                && (p.rot[1] + 10.0).abs() < 1e-3
                && (p.rot[2] - 5.0).abs() < 1e-3
        );
        assert_eq!(p.pos, [1.5, -2.0, 10.0]);
    }

    #[test]
    fn npclient_pose_is_in_cm_and_degrees() {
        // (TrackIR's +-16383: half of it is 90 degrees, 25 cm)
        let half = 16383.0 / 2.0;
        let p = npclient_pose(half / 9.0, -half / 9.0, half / 3.0, half, -half / 5.0, 0.0).unwrap();
        // (yaw and pitch the other way round, as from the freetrack mapping; TrackIR's roll
        // and sideways lean too)
        let want = [-30.0, 10.0, -10.0];
        assert!(
            p.rot.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-3),
            "{p:?}"
        );
        let want = [-25.0, -5.0, 0.0];
        assert!(
            p.pos.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4),
            "{p:?}"
        );
    }

    #[test]
    fn npclient_pose_drops_what_no_tracker_sends() {
        assert!(npclient_pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0).is_some());
        assert!(npclient_pose(16383.0, -16383.0, 16383.0, 16383.0, -16383.0, 16383.0).is_some());
        assert!(npclient_pose(0.0, 0.0, 3.0e7, 0.0, 0.0, 0.0).is_none());
        assert!(npclient_pose(0.0, f32::NAN, 0.0, 0.0, 0.0, 0.0).is_none());
        assert!(npclient_pose(0.0, 0.0, 0.0, f32::INFINITY, 0.0, 0.0).is_none());
    }

    /// The whole TrackIR path against opentrack's NPClient stand-in, without a tracker: the
    /// test plays opentrack's part and writes poses to `FT_SharedMem`, which its
    /// NPClient64.dll hands on through the TrackIR calls. Needs opentrack's NPClient registered
    /// (opentrack does so on its first run): `cargo test -p core headtrack -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore = "needs opentrack's NPClient registered (not TrackIR's own)"]
    fn trackir_through_opentracks_npclient() {
        use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_WRITE, MapViewOfFile, PAGE_READWRITE,
        };
        use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

        // SAFETY: a mapping of this test's own, written while NPClient reads it.
        let base = unsafe {
            let h = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                4096,
                windows::core::w!("FT_SharedMem"),
            )
            .unwrap();
            MapViewOfFile(h, FILE_MAP_WRITE, 0, 0, 4096).Value as *mut u32
        };
        assert!(!base.is_null());
        let write = |id: u32, v: [f32; 6]| {
            // SAFETY: within the view; FTData: DataID, CamWidth, CamHeight, Yaw..Z.
            unsafe {
                for (i, x) in v.iter().enumerate() {
                    base.add(3 + i).write_volatile(x.to_bits());
                }
                base.write_volatile(id);
            }
        };
        let hwnd = unsafe { GetDesktopWindow() }.0 as isize;
        let mut reader = npclient::Reader::new(hwnd);
        // yaw 30, pitch -10, roll 5 degrees, x 15, y -20, z 100 mm (the freetrack test's)
        let pose = [
            -30f32.to_radians(),
            10f32.to_radians(),
            5f32.to_radians(),
            15.0,
            -20.0,
            100.0,
        ];
        let mut got = None;
        for id in 1..200 {
            write(id, pose);
            if let Some(p) = reader.poll() {
                got = Some(p);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let p = got.expect("no pose through NPClient");
        // (opentrack hands roll and x on as they are, so they come out the other way round to
        // the freetrack mapping's: why the mapping wins while opentrack writes it)
        let want = freetrack_pose(pose[0], pose[1], -pose[2], -pose[3], pose[4], pose[5]);
        // (NPClient's +-16383 steps lose a little)
        assert!(
            p.rot
                .iter()
                .zip(want.rot)
                .all(|(a, b)| (a - b).abs() < 0.05)
                && p.pos
                    .iter()
                    .zip(want.pos)
                    .all(|(a, b)| (a - b).abs() < 0.01),
            "{p:?} vs {want:?}"
        );
        // a new frame comes through as such, an unchanged one does not
        write(500, [0.0, 0.0, 0.0, 50.0, 0.0, 0.0]);
        let mut next = None;
        for _ in 0..50 {
            next = reader.poll();
            if next.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let next = next.expect("no second pose");
        assert!(
            (next.pos[0] + 5.0).abs() < 0.01 && next.rot[0].abs() < 0.05,
            "{next:?}"
        );
        drop(reader);
    }
}
