mod commands;
mod minimap;
mod pads;

use launcher_protocol::{self as protocol, Message};
use launcher_protocol::link;
use omsi_launcher_lib::Instance;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const QUIET: Duration = Duration::from_millis(1000);
const PAX_RELEASES_EVERY: Duration = Duration::from_secs(6 * 3600);
const INSTALLING: Duration = Duration::from_millis(250);
/// A `launch` cut off between spawning the game and writing its file would lose the game.
const FINISH_REQUESTS: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(crate) struct Server(Arc<Inner>);

struct Inner {
    out: Mutex<Box<dyn Write + Send>>,
    ready: AtomicBool,
    stop: AtomicBool,
    busy: AtomicUsize,
    wake: Mutex<Sender<()>>,
    game_link: AtomicBool,
}

pub(crate) fn run() -> anyhow::Result<()> {
    let out = take_stdout()?;
    omsi_launcher_lib::init_settings();
    omsi_launcher_lib::cleanup();
    let (server, woken) = Server::new(Box::new(out));
    let s = server.clone();
    match link::listen(move |_| s.wake()) {
        Ok(addr) => {
            log::info!("game link on {addr}");
            server.0.game_link.store(true, Ordering::SeqCst);
        }
        Err(e) => log::warn!("no game link ({e}): games are seen through their files only"),
    }
    server.watch(woken);
    let _ = std::thread::Builder::new()
        .name("pax releases".into())
        .spawn(|| {
            loop {
                crate::pax_pack::refresh();
                std::thread::sleep(PAX_RELEASES_EVERY);
            }
        });
    server.serve(std::io::stdin().lock());
    log::info!("the launcher went away: the engine ends (games keep running)");
    Ok(())
}

/// A stray `println!` would break the framing: stdout becomes stderr, the frames keep the
/// original handle.
#[cfg(unix)]
fn take_stdout() -> std::io::Result<std::fs::File> {
    use std::os::fd::FromRawFd;
    let _ = std::io::stdout().flush();
    unsafe {
        // (close-on-exec: a game holding it open would hide the engine's end from the launcher)
        let fd = libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 3);
        if fd < 0 || libc::dup2(2, 1) < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(std::fs::File::from_raw_fd(fd))
    }
}

#[cfg(windows)]
fn take_stdout() -> std::io::Result<std::fs::File> {
    use std::os::windows::io::FromRawHandle;
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> isize;
        fn SetStdHandle(which: u32, handle: isize) -> i32;
        fn SetHandleInformation(handle: isize, mask: u32, flags: u32) -> i32;
    }
    const STD_INPUT: u32 = -10i32 as u32;
    const STD_OUTPUT: u32 = -11i32 as u32;
    const STD_ERROR: u32 = -12i32 as u32;
    const HANDLE_FLAG_INHERIT: u32 = 1;
    let _ = std::io::stdout().flush();
    // SAFETY: our own standard handles; std looks stdout up on every write, so it follows
    unsafe {
        let out = GetStdHandle(STD_OUTPUT);
        if out == 0 || out == -1 {
            return Err(std::io::Error::other("no standard output to answer on"));
        }
        // the launcher's pipes, inherited by every game, would outlive the engine
        for h in [GetStdHandle(STD_INPUT), out, GetStdHandle(STD_ERROR)] {
            if h != 0 && h != -1 {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
        SetStdHandle(STD_OUTPUT, GetStdHandle(STD_ERROR));
        Ok(std::fs::File::from_raw_handle(out as *mut std::ffi::c_void))
    }
}

impl Server {
    pub(crate) fn new(out: Box<dyn Write + Send>) -> (Server, Receiver<()>) {
        let (tx, rx) = channel();
        let server = Server(Arc::new(Inner {
            out: Mutex::new(out),
            ready: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            busy: AtomicUsize::new(0),
            wake: Mutex::new(tx),
            game_link: AtomicBool::new(false),
        }));
        (server, rx)
    }

    fn wake(&self) {
        let _ = self.0.wake.lock().unwrap_or_else(|e| e.into_inner()).send(());
    }

    fn send(&self, m: &Message) {
        let frame = match protocol::encode(m) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("launcher protocol: {} not sent: {e}", m.kind);
                if m.request_id.is_some() && m.error.is_none() {
                    self.send(&Message {
                        kind: m.kind.clone(),
                        request_id: m.request_id.clone(),
                        payload: Value::Null,
                        error: Some(format!("the answer could not be sent: {e}")),
                    });
                }
                return;
            }
        };
        let mut out = self.0.out.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = out.write_all(&frame).and_then(|()| out.flush()) {
            log::warn!("launcher protocol: {} not sent: {e}", m.kind);
            self.0.stop.store(true, Ordering::SeqCst);
        }
    }

    pub(crate) fn serve(&self, mut input: impl Read) {
        loop {
            let m = match protocol::read_frame(&mut input) {
                Ok(Some(m)) => m,
                Ok(None) => break,
                Err(e) => {
                    log::error!("launcher protocol: {e}");
                    break;
                }
            };
            match m.kind.as_str() {
                "handshake" => {
                    let r = self.handshake(&m.payload);
                    self.send(&m.reply(r));
                    self.wake();
                }
                "shutdown" => {
                    self.send(&m.reply(Ok(json!({}))));
                    break;
                }
                _ if !self.0.ready.load(Ordering::SeqCst) => {
                    self.send(&m.reply(Err("the handshake has to come first".into())));
                }
                kind if !commands::COMMANDS.contains(&kind) => {
                    self.send(&m.reply(Err(format!("this engine has no command {kind:?}"))));
                }
                _ => {
                    let this = self.clone();
                    self.0.busy.fetch_add(1, Ordering::SeqCst);
                    let spawned = std::thread::Builder::new()
                        .name("launcher request".into())
                        .spawn(move || {
                            let r = commands::call(&m.kind, &m.payload).map_err(|e| format!("{e:#}"));
                            this.send(&m.reply(r));
                            this.0.busy.fetch_sub(1, Ordering::SeqCst);
                            this.wake();
                        });
                    if let Err(e) = spawned {
                        self.0.busy.fetch_sub(1, Ordering::SeqCst);
                        log::error!("launcher protocol: no thread for a request: {e}");
                    }
                }
            }
            if self.0.stop.load(Ordering::SeqCst) {
                break;
            }
        }
        let until = Instant::now() + FINISH_REQUESTS;
        while self.0.busy.load(Ordering::SeqCst) > 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        self.0.stop.store(true, Ordering::SeqCst);
        self.wake();
    }

    fn handshake(&self, p: &Value) -> Result<Value, String> {
        let theirs = p.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("");
        let engine = crate::startup::VERSION;
        if theirs.split('.').next() != Some(protocol::VERSION) {
            self.0.ready.store(false, Ordering::SeqCst);
            return Ok(json!({
                "status": {
                    "code": 2,
                    "message": format!("this engine speaks protocol {}, the launcher {theirs:?}", protocol::VERSION),
                },
                "protocolVersion": protocol::VERSION,
                "engineVersion": engine,
                "supportedCapabilities": [],
                "commands": [],
            }));
        }
        log::info!(
            "launcher {} on {} connected",
            p.get("launcherVersion").and_then(|v| v.as_str()).unwrap_or("?"),
            p.get("clientPlatform").and_then(|v| v.as_str()).unwrap_or("?"),
        );
        self.0.ready.store(true, Ordering::SeqCst);
        let mut caps = vec![
            "events.instances",
            "events.installs",
            "events.content",
            "events.session",
        ];
        if self.0.game_link.load(Ordering::SeqCst) {
            caps.push("game.link");
        }
        Ok(json!({
            "status": { "code": 0, "message": "OK" },
            "protocolVersion": protocol::VERSION,
            "engineVersion": engine,
            "supportedCapabilities": caps,
            "commands": commands::COMMANDS,
        }))
    }

    pub(crate) fn watch(&self, woken: Receiver<()>) {
        let this = self.clone();
        let spawned = std::thread::Builder::new()
            .name("launcher events".into())
            .spawn(move || {
                let mut seen = Seen::default();
                loop {
                    if this.0.stop.load(Ordering::SeqCst) {
                        return;
                    }
                    if this.0.ready.load(Ordering::SeqCst) {
                        match omsi_launcher_lib::poll() {
                            Ok(p) => {
                                for m in seen.update(&p.stamp, &p.jobs, &p.instances) {
                                    this.send(&m);
                                }
                                if let Some(m) = seen.pax(commands::pax_status()) {
                                    this.send(&m);
                                }
                            }
                            Err(e) => log::debug!("poll: {e:#}"),
                        }
                    }
                    let wait = if seen.installing { INSTALLING } else { QUIET };
                    match woken.recv_timeout(wait) {
                        Ok(()) => while woken.try_recv().is_ok() {},
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            });
        if let Err(e) = spawned {
            log::error!("launcher events: {e}");
        }
    }
}

/// The launcher's SessionState numbers.
mod session {
    pub const STARTING: u8 = 1;
    pub const LOADING: u8 = 2;
    pub const RUNNING: u8 = 3;
    pub const STOPPING: u8 = 4;
    pub const EXITED: u8 = 5;
    pub const FAILED: u8 = 6;
}

/// An older game never reports: running once it has been up this long.
const UNLINKED_START_SECS: u64 = 30;

#[derive(Clone, Debug, PartialEq)]
struct Phase {
    state: u8,
    message: String,
    progress: Option<f32>,
    exit_code: Option<i32>,
}

#[derive(Default)]
struct Seen {
    started: bool,
    stamp: String,
    jobs: Value,
    instances: Value,
    phases: HashMap<String, Phase>,
    installing: bool,
    pax: Value,
}

impl Seen {
    fn pax(&mut self, status: Value) -> Option<Message> {
        self.installing |= matches!(status["state"].as_str(), Some("downloading" | "installing"));
        if status == self.pax {
            return None;
        }
        self.pax = status.clone();
        Some(Message::new("pax_pack_changed", status))
    }

    fn update(
        &mut self,
        stamp: &str,
        jobs: &[omsi_launcher_lib::install::Progress],
        instances: &[Instance],
    ) -> Vec<Message> {
        let mut out = Vec::new();
        let first = !self.started;
        self.started = true;
        if !first && stamp != self.stamp {
            commands::forget_content();
            out.push(Message::new("content_changed", json!({ "stamp": stamp })));
        }
        self.stamp = stamp.to_string();
        self.installing = jobs.iter().any(|j| j.finished.is_none());
        let jobs = serde_json::to_value(jobs).unwrap_or_default();
        if jobs != self.jobs {
            self.jobs = jobs.clone();
            out.push(Message::new("installs_changed", jobs));
        }
        let list = serde_json::to_value(instances).unwrap_or_default();
        if list != self.instances {
            self.instances = list.clone();
            out.push(Message::new("instances_changed", list));
        }
        let now = omsi_launcher_lib::install::now_secs();
        for i in instances {
            let before = self.phases.get(&i.id);
            let phase = phase_of(i, before, now);
            if before == Some(&phase) {
                continue;
            }
            if !(first && phase.state >= session::EXITED) {
                let mut e = json!({
                    "sessionId": i.id,
                    "pid": i.pid,
                    "state": phase.state,
                    "message": phase.message,
                });
                if let Some(p) = phase.progress {
                    e["progress"] = json!(p);
                }
                if let Some(c) = phase.exit_code {
                    e["exitCode"] = json!(c);
                }
                out.push(Message::new("session_event", e));
            }
            self.phases.insert(i.id.clone(), phase);
        }
        self.phases.retain(|id, _| instances.iter().any(|i| &i.id == id));
        out
    }
}

fn phase_of(i: &Instance, before: Option<&Phase>, now: u64) -> Phase {
    let phase = |state, message: &str, progress| Phase {
        state,
        message: message.to_string(),
        progress,
        exit_code: None,
    };
    if !i.running {
        if let Some(b) = before.filter(|b| b.state == session::FAILED) {
            return Phase {
                exit_code: i.exit_code,
                ..b.clone()
            };
        }
        let failed = !i.killed && i.exit_code.is_some_and(|c| c != 0);
        return Phase {
            exit_code: i.exit_code,
            ..phase(
                if failed { session::FAILED } else { session::EXITED },
                &i.last_line,
                None,
            )
        };
    }
    if i.stopping.is_some() {
        return phase(session::STOPPING, "", None);
    }
    match &i.link {
        Some(l) => {
            let state = match l.state.as_str() {
                "loading" => session::LOADING,
                "running" => session::RUNNING,
                "stopping" => session::STOPPING,
                "failed" => session::FAILED,
                _ => session::STARTING,
            };
            phase(state, &l.message, l.progress)
        }
        None if now.saturating_sub(i.started) < UNLINKED_START_SECS => {
            phase(session::STARTING, "", None)
        }
        None => before
            .filter(|b| b.state != session::STARTING)
            .cloned()
            .unwrap_or_else(|| phase(session::RUNNING, "", None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher_protocol::link::GameState;

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn frames(m: &[Message]) -> Vec<u8> {
        m.iter().flat_map(|m| protocol::encode(m).unwrap()).collect()
    }

    fn request(kind: &str, id: &str, payload: Value) -> Message {
        Message {
            kind: kind.into(),
            request_id: Some(id.into()),
            payload,
            error: None,
        }
    }

    fn answers(out: &Shared) -> Vec<Message> {
        let bytes = out.0.lock().unwrap().clone();
        let mut r = std::io::Cursor::new(bytes);
        std::iter::from_fn(|| protocol::read_frame(&mut r).unwrap()).collect()
    }

    #[test]
    fn the_handshake_comes_first_and_answers_carry_the_request_id() {
        let out = Shared::default();
        let (server, _woken) = Server::new(Box::new(out.clone()));
        server.serve(std::io::Cursor::new(frames(&[
            request("version", "a", json!({})),
            request("handshake", "b", json!({ "protocolVersion": "1.0", "launcherVersion": "0.3.0" })),
            request("shutdown", "c", json!({})),
            request("version", "d", json!({})),
        ])));
        let got = answers(&out);
        assert_eq!(got.len(), 3, "nothing after shutdown: {got:?}");
        assert_eq!(got[0].request_id.as_deref(), Some("a"));
        assert!(got[0].error.as_deref().unwrap().contains("handshake"));
        assert_eq!(got[1].payload["status"]["code"], 0);
        assert_eq!(got[1].payload["protocolVersion"], protocol::VERSION);
        assert!(
            got[1].payload["commands"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c == "launch")
        );
        assert_eq!(got[2].kind, "shutdown");
    }

    #[test]
    fn another_major_version_is_refused() {
        let out = Shared::default();
        let (server, _woken) = Server::new(Box::new(out.clone()));
        server.serve(std::io::Cursor::new(frames(&[
            request("handshake", "a", json!({ "protocolVersion": "2.0" })),
            request("config", "b", json!({})),
        ])));
        let got = answers(&out);
        assert_eq!(got[0].payload["status"]["code"], 2);
        assert!(got[1].error.is_some(), "still not ready");
    }

    #[test]
    fn requests_run_side_by_side() {
        let out = Shared::default();
        let (server, _woken) = Server::new(Box::new(out.clone()));
        server.0.ready.store(true, Ordering::SeqCst);
        server.serve(std::io::Cursor::new(frames(&[
            request("version", "1", json!({})),
            request("teleport", "2", json!({})),
        ])));
        let got = (0..100)
            .map(|_| {
                std::thread::sleep(Duration::from_millis(20));
                answers(&out)
            })
            .find(|a| a.len() == 2)
            .expect("both answered");
        let by = |id: &str| got.iter().find(|m| m.request_id.as_deref() == Some(id)).unwrap();
        assert_eq!(by("1").payload["protocol"], 1);
        assert!(by("2").error.as_deref().unwrap().contains("teleport"));
    }

    #[test]
    fn a_bad_request_type_is_answered_and_the_engine_goes_on() {
        let out = Shared::default();
        let (server, _woken) = Server::new(Box::new(out.clone()));
        server.0.ready.store(true, Ordering::SeqCst);
        server.serve(std::io::Cursor::new(frames(&[
            request("a\u{0}b", "1", json!({})),
            request("version", "2", json!({})),
        ])));
        let got = answers(&out);
        assert!(got[0].error.as_deref().unwrap().contains("no command"));
        assert_eq!(got[1].payload["protocol"], 1, "answered before the engine ended");
    }

    fn game(id: &str, running: bool) -> Instance {
        Instance {
            id: id.into(),
            pid: 7,
            started: omsi_launcher_lib::install::now_secs(),
            running,
            ..Default::default()
        }
    }

    fn kinds(m: &[Message]) -> Vec<(String, Value)> {
        m.iter()
            .map(|m| (m.kind.clone(), m.payload["state"].clone()))
            .collect()
    }

    #[test]
    fn a_game_moves_through_its_session_states() {
        let mut seen = Seen::default();
        let mut g = game("g", true);
        let first = seen.update("s1", &[], std::slice::from_ref(&g));
        assert_eq!(
            kinds(&first),
            [
                ("installs_changed".into(), Value::Null),
                ("instances_changed".into(), Value::Null),
                ("session_event".into(), json!(session::STARTING)),
            ]
        );
        assert!(seen.update("s1", &[], std::slice::from_ref(&g)).is_empty());

        g.link = Some(GameState {
            state: "loading".into(),
            progress: Some(0.25),
            message: "Spandau".into(),
            window: true,
        });
        let m = seen.update("s1", &[], std::slice::from_ref(&g));
        let e = &m.iter().find(|m| m.kind == "session_event").unwrap().payload;
        assert_eq!((e["state"].clone(), e["progress"].clone()), (json!(session::LOADING), json!(0.25)));

        g.link = Some(GameState {
            state: "failed".into(),
            progress: None,
            message: "the map did not load".into(),
            window: true,
        });
        seen.update("s1", &[], std::slice::from_ref(&g));
        g.running = false;
        g.link = None;
        g.exit_code = Some(0);
        let m = seen.update("s2", &[], std::slice::from_ref(&g));
        assert_eq!(m[0].kind, "content_changed");
        let e = &m.iter().find(|m| m.kind == "session_event").unwrap().payload;
        assert_eq!(e["state"], session::FAILED, "the game's own report outlives it");
        assert_eq!(e["message"], "the map did not load");
        assert_eq!(e["exitCode"], 0);
    }

    #[test]
    fn exits_are_told_apart_and_old_games_are_not_announced() {
        let mut seen = Seen::default();
        let mut old = game("old", false);
        old.exit_code = Some(1);
        let m = seen.update("s", &[], &[old.clone()]);
        assert!(!m.iter().any(|m| m.kind == "session_event"));

        let mut crashed = game("crashed", true);
        let mut stopped = game("stopped", true);
        seen.update("s", &[], &[old.clone(), crashed.clone(), stopped.clone()]);
        crashed.running = false;
        crashed.exit_code = Some(-1073741819);
        stopped.running = false;
        stopped.killed = true;
        stopped.exit_code = Some(1);
        let m = seen.update("s", &[], &[old, crashed, stopped]);
        let state = |id: &str| {
            m.iter()
                .find(|m| m.kind == "session_event" && m.payload["sessionId"] == id)
                .map(|m| m.payload["state"].clone())
        };
        assert_eq!(state("crashed"), Some(json!(session::FAILED)));
        assert_eq!(state("stopped"), Some(json!(session::EXITED)), "Stop had to kill it");
        assert_eq!(state("old"), None);
    }
}
