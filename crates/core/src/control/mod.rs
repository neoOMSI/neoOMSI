mod commands;
mod minimap;
mod pads;

use launcher_protocol::api::{
    self, Empty, Frame, GameLinkState, HandshakeResponse, PaxState, SessionState, Status, StatusCode, event::Event, frame::Body,
    request::Command, response::Answer,
};
use launcher_protocol::link;
use omsi_launcher_lib::Instance;
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

    fn send(&self, f: &Frame) {
        let bytes = match launcher_protocol::encode(f) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("launcher protocol: a frame not sent: {e}");
                if !f.request_id.is_empty() && f.error.is_empty() {
                    self.send(&failed(&f.request_id, format!("the answer could not be sent: {e}")));
                }
                return;
            }
        };
        let mut out = self.0.out.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = out.write_all(&bytes).and_then(|()| out.flush()) {
            log::warn!("launcher protocol: a frame not sent: {e}");
            self.0.stop.store(true, Ordering::SeqCst);
        }
    }

    pub(crate) fn serve(&self, mut input: impl Read) {
        loop {
            let frame = match launcher_protocol::read_frame::<Frame>(&mut input) {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(e) => {
                    log::error!("launcher protocol: {e}");
                    break;
                }
            };
            let id = frame.request_id;
            let command = match frame.body {
                Some(Body::Request(r)) => r.command,
                _ => None,
            };
            match command {
                Some(Command::Handshake(h)) => {
                    let r = self.handshake(&h);
                    self.send(&answer(&id, Answer::Handshake(r)));
                    self.wake();
                }
                Some(Command::Shutdown(_)) => {
                    self.send(&answer(&id, Answer::Shutdown(Empty {})));
                    break;
                }
                _ if !self.0.ready.load(Ordering::SeqCst) => {
                    self.send(&failed(&id, "the handshake has to come first".into()));
                }
                None => {
                    self.send(&failed(&id, "this engine does not know the request".into()));
                }
                Some(command) => {
                    let this = self.clone();
                    self.0.busy.fetch_add(1, Ordering::SeqCst);
                    let spawned = std::thread::Builder::new()
                        .name("launcher request".into())
                        .spawn(move || {
                            let reply = match commands::call(command) {
                                Ok(a) => answer(&id, a),
                                Err(e) => failed(&id, format!("{e:#}")),
                            };
                            this.send(&reply);
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

    fn handshake(&self, h: &api::Handshake) -> HandshakeResponse {
        let theirs = &h.protocol_version;
        let response = |code: StatusCode, message: String, caps: &[&str], commands: &[&str]| HandshakeResponse {
            status: Some(Status {
                code: code.into(),
                message,
            }),
            protocol_version: api::VERSION.into(),
            engine_version: crate::startup::VERSION.into(),
            supported_capabilities: caps.iter().map(|c| c.to_string()).collect(),
            commands: commands.iter().map(|c| c.to_string()).collect(),
        };
        if theirs.split('.').next() != Some(api::VERSION) {
            self.0.ready.store(false, Ordering::SeqCst);
            let message = format!("this engine speaks protocol {}, the launcher {theirs:?}", api::VERSION);
            return response(StatusCode::UnsupportedVersion, message, &[], &[]);
        }
        let known = |s: &str| if s.is_empty() { "?" } else { s }.to_string();
        log::info!(
            "launcher {} on {} connected",
            known(&h.launcher_version),
            known(&h.client_platform),
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
        response(StatusCode::Ok, "OK".into(), &caps, api::COMMANDS)
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
                                for e in seen.update(&p.stamp, &p.jobs, &p.instances) {
                                    this.send(&event(e));
                                }
                                if let Some(e) = seen.pax(commands::pax_status()) {
                                    this.send(&event(e));
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

fn answer(id: &str, a: Answer) -> Frame {
    Frame {
        request_id: id.into(),
        error: String::new(),
        body: Some(Body::Response(api::Response { answer: Some(a) })),
    }
}

fn failed(id: &str, error: String) -> Frame {
    Frame {
        request_id: id.into(),
        error,
        body: None,
    }
}

fn event(e: Event) -> Frame {
    Frame {
        body: Some(Body::Event(api::Event { event: Some(e) })),
        ..Default::default()
    }
}

/// An older game never reports: running once it has been up this long.
const UNLINKED_START_SECS: u64 = 30;

#[derive(Clone, Debug, PartialEq)]
struct Phase {
    state: SessionState,
    message: String,
    progress: Option<f32>,
    exit_code: Option<i32>,
}

#[derive(Default)]
struct Seen {
    started: bool,
    stamp: String,
    jobs: Option<Vec<api::InstallProgress>>,
    instances: Option<Vec<api::Instance>>,
    phases: HashMap<String, Phase>,
    installing: bool,
    pax: Option<api::PaxPack>,
}

impl Seen {
    fn pax(&mut self, status: api::PaxPack) -> Option<Event> {
        self.installing |= matches!(status.state(), PaxState::Downloading | PaxState::Installing);
        if self.pax.as_ref() == Some(&status) {
            return None;
        }
        self.pax = Some(status.clone());
        Some(Event::PaxPackChanged(status))
    }

    fn update(
        &mut self,
        stamp: &str,
        jobs: &[omsi_launcher_lib::install::Progress],
        instances: &[Instance],
    ) -> Vec<Event> {
        let mut out = Vec::new();
        let first = !self.started;
        self.started = true;
        if !first && stamp != self.stamp {
            commands::forget_content();
            out.push(Event::ContentChanged(api::ContentChanged { stamp: stamp.into() }));
        }
        self.stamp = stamp.to_string();
        self.installing = jobs.iter().any(|j| j.finished.is_none());
        let jobs: Vec<api::InstallProgress> = jobs.iter().cloned().map(Into::into).collect();
        if self.jobs.as_ref() != Some(&jobs) {
            self.jobs = Some(jobs.clone());
            out.push(Event::InstallsChanged(api::InstallList { jobs }));
        }
        let list: Vec<api::Instance> = instances.iter().cloned().map(Into::into).collect();
        if self.instances.as_ref() != Some(&list) {
            self.instances = Some(list.clone());
            out.push(Event::InstancesChanged(api::InstanceList { instances: list }));
        }
        let now = omsi_launcher_lib::install::now_secs();
        for i in instances {
            let before = self.phases.get(&i.id);
            let phase = phase_of(i, before, now);
            if before == Some(&phase) {
                continue;
            }
            if !(first && phase.state >= SessionState::Exited) {
                out.push(Event::SessionEvent(api::SessionEvent {
                    session_id: i.id.clone(),
                    pid: i.pid,
                    state: phase.state.into(),
                    message: phase.message.clone(),
                    progress: phase.progress,
                    exit_code: phase.exit_code,
                }));
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
        if let Some(b) = before.filter(|b| b.state == SessionState::Failed) {
            return Phase {
                exit_code: i.exit_code,
                ..b.clone()
            };
        }
        let failed = !i.killed && i.exit_code.is_some_and(|c| c != 0);
        return Phase {
            exit_code: i.exit_code,
            ..phase(
                if failed { SessionState::Failed } else { SessionState::Exited },
                &i.last_line,
                None,
            )
        };
    }
    if i.stopping.is_some() {
        return phase(SessionState::Stopping, "", None);
    }
    match &i.link {
        Some(l) => {
            let state = match l.state() {
                GameLinkState::Loading => SessionState::Loading,
                GameLinkState::Running => SessionState::Running,
                GameLinkState::Stopping => SessionState::Stopping,
                GameLinkState::Failed => SessionState::Failed,
                GameLinkState::Starting | GameLinkState::Unspecified => SessionState::Starting,
            };
            phase(state, &l.message, l.progress)
        }
        None if now.saturating_sub(i.started) < UNLINKED_START_SECS => {
            phase(SessionState::Starting, "", None)
        }
        None => before
            .filter(|b| b.state != SessionState::Starting)
            .cloned()
            .unwrap_or_else(|| phase(SessionState::Running, "", None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher_protocol::api::GameLink;

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

    fn frames(f: &[Frame]) -> Vec<u8> {
        f.iter().flat_map(|f| launcher_protocol::encode(f).unwrap()).collect()
    }

    fn request(id: &str, command: Option<Command>) -> Frame {
        Frame {
            request_id: id.into(),
            body: Some(Body::Request(api::Request { command })),
            ..Default::default()
        }
    }

    fn handshake(version: &str) -> Option<Command> {
        Some(Command::Handshake(api::Handshake {
            protocol_version: version.into(),
            launcher_version: "0.3.0".into(),
            client_platform: "win32".into(),
        }))
    }

    fn version() -> Option<Command> {
        Some(Command::Version(Empty {}))
    }

    fn answers(out: &Shared) -> Vec<Frame> {
        let bytes = out.0.lock().unwrap().clone();
        let mut r = std::io::Cursor::new(bytes);
        std::iter::from_fn(|| launcher_protocol::read_frame(&mut r).unwrap()).collect()
    }

    fn answer_of(f: &Frame) -> &Answer {
        match &f.body {
            Some(Body::Response(api::Response { answer: Some(a) })) => a,
            _ => panic!("not an answer: {f:?}"),
        }
    }

    #[test]
    fn the_handshake_comes_first_and_answers_carry_the_request_id() {
        let out = Shared::default();
        let (server, _woken) = Server::new(Box::new(out.clone()));
        server.serve(std::io::Cursor::new(frames(&[
            request("a", version()),
            request("b", handshake("2.0")),
            request("c", Some(Command::Shutdown(Empty {}))),
            request("d", version()),
        ])));
        let got = answers(&out);
        assert_eq!(got.len(), 3, "nothing after shutdown: {got:?}");
        assert_eq!(got[0].request_id, "a");
        assert!(got[0].error.contains("handshake"));
        let Answer::Handshake(h) = answer_of(&got[1]) else {
            panic!("not the handshake")
        };
        assert_eq!(h.status.as_ref().unwrap().code(), StatusCode::Ok);
        assert_eq!(h.protocol_version, api::VERSION);
        assert!(h.commands.iter().any(|c| c == "launch"));
        assert_eq!(answer_of(&got[2]), &Answer::Shutdown(Empty {}));
    }

    #[test]
    fn another_major_version_is_refused() {
        let out = Shared::default();
        let (server, _woken) = Server::new(Box::new(out.clone()));
        server.serve(std::io::Cursor::new(frames(&[
            request("a", handshake("1.0")),
            request("b", Some(Command::Config(Empty {}))),
        ])));
        let got = answers(&out);
        let Answer::Handshake(h) = answer_of(&got[0]) else {
            panic!("not the handshake")
        };
        assert_eq!(h.status.as_ref().unwrap().code(), StatusCode::UnsupportedVersion);
        assert!(h.commands.is_empty());
        assert!(!got[1].error.is_empty(), "still not ready");
    }

    #[test]
    fn a_launcher_of_the_first_protocol_ends_the_connection() {
        let out = Shared::default();
        let (server, _woken) = Server::new(Box::new(out.clone()));
        let json = launcher_protocol::frame(br#"{"type":"handshake","requestId":"a","payload":{}}"#);
        server.serve(std::io::Cursor::new(json.unwrap()));
        assert!(answers(&out).is_empty());
    }

    #[test]
    fn requests_run_side_by_side_and_an_unknown_one_is_answered() {
        let out = Shared::default();
        let (server, _woken) = Server::new(Box::new(out.clone()));
        server.0.ready.store(true, Ordering::SeqCst);
        server.serve(std::io::Cursor::new(frames(&[request("1", None), request("2", version())])));
        let got = (0..100)
            .map(|_| {
                std::thread::sleep(Duration::from_millis(20));
                answers(&out)
            })
            .find(|a| a.len() == 2)
            .expect("both answered");
        let by = |id: &str| got.iter().find(|f| f.request_id == id).unwrap();
        assert!(by("1").error.contains("does not know"));
        let Answer::Version(v) = answer_of(by("2")) else {
            panic!("not the version")
        };
        assert_eq!(v.protocol, 2);
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

    fn session_events(events: &[Event]) -> Vec<&api::SessionEvent> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::SessionEvent(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_game_moves_through_its_session_states() {
        let mut seen = Seen::default();
        let mut g = game("g", true);
        let first = seen.update("s1", &[], std::slice::from_ref(&g));
        assert!(matches!(
            first.as_slice(),
            [Event::InstallsChanged(_), Event::InstancesChanged(_), Event::SessionEvent(e)]
                if e.state() == SessionState::Starting
        ));
        assert!(seen.update("s1", &[], std::slice::from_ref(&g)).is_empty());

        g.link = Some(GameLink {
            state: GameLinkState::Loading.into(),
            progress: Some(0.25),
            message: "Spandau".into(),
            window: true,
        });
        let m = seen.update("s1", &[], std::slice::from_ref(&g));
        let e = session_events(&m)[0];
        assert_eq!((e.state(), e.progress), (SessionState::Loading, Some(0.25)));

        g.link = Some(GameLink {
            state: GameLinkState::Failed.into(),
            progress: None,
            message: "the map did not load".into(),
            window: true,
        });
        seen.update("s1", &[], std::slice::from_ref(&g));
        g.running = false;
        g.link = None;
        g.exit_code = Some(0);
        let m = seen.update("s2", &[], std::slice::from_ref(&g));
        assert!(matches!(&m[0], Event::ContentChanged(c) if c.stamp == "s2"));
        let e = session_events(&m)[0];
        assert_eq!(e.state(), SessionState::Failed, "the game's own report outlives it");
        assert_eq!(e.message, "the map did not load");
        assert_eq!(e.exit_code, Some(0));
    }

    #[test]
    fn exits_are_told_apart_and_old_games_are_not_announced() {
        let mut seen = Seen::default();
        let mut old = game("old", false);
        old.exit_code = Some(1);
        let m = seen.update("s", &[], &[old.clone()]);
        assert!(session_events(&m).is_empty());

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
            session_events(&m)
                .into_iter()
                .find(|e| e.session_id == id)
                .map(|e| e.state())
        };
        assert_eq!(state("crashed"), Some(SessionState::Failed));
        assert_eq!(state("stopped"), Some(SessionState::Exited), "Stop had to kill it");
        assert_eq!(state("old"), None);
    }

    #[test]
    fn the_pack_is_announced_when_it_changes() {
        let mut seen = Seen::default();
        let pack = api::PaxPack {
            state: PaxState::Downloading.into(),
            done: 1,
            total: 4,
            ..Default::default()
        };
        assert!(matches!(seen.pax(pack.clone()), Some(Event::PaxPackChanged(_))));
        assert!(seen.installing);
        assert_eq!(seen.pax(pack), None);
    }
}
