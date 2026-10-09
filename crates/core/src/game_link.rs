use launcher_protocol::link::{ENV_ADDR, ENV_TOKEN};
use launcher_protocol::{self as protocol, Message};
use serde_json::json;
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Conn {
    stream: TcpStream,
    last: (String, Instant),
    window: bool,
}

static CONN: Mutex<Option<Conn>> = Mutex::new(None);

const PROGRESS_EVERY: Duration = Duration::from_millis(250);

pub(crate) fn connect() {
    let (Ok(addr), Ok(token), Ok(instance)) = (
        legacy_config::env::var(ENV_ADDR),
        legacy_config::env::var(ENV_TOKEN),
        legacy_config::env::var("OMSI_INSTANCE"),
    ) else {
        return;
    };
    match open(&addr, &token, &instance) {
        Ok(()) => log::info!("launcher link: connected to {addr}"),
        Err(e) => log::warn!("launcher link: {e} (the launcher only sees the log)"),
    }
}

fn open(addr: &str, token: &str, instance: &str) -> anyhow::Result<()> {
    let addr = addr.parse()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    protocol::write_frame(
        &mut stream,
        &Message::new(
            "hello",
            json!({ "instance": instance, "token": token, "pid": std::process::id(), "version": crate::startup::VERSION }),
        ),
    )?;
    match protocol::read_frame(&mut stream)? {
        Some(m) if m.kind == "welcome" => {}
        Some(m) => anyhow::bail!("the engine answered {}", m.kind),
        None => anyhow::bail!("the engine hung up"),
    }
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(Some(Duration::from_millis(200)))?;
    let mut reader = stream.try_clone()?;
    std::thread::Builder::new()
        .name("launcher link".into())
        .spawn(move || {
            while let Ok(Some(m)) = protocol::read_frame(&mut reader) {
                if m.kind == "quit" {
                    crate::quit::request();
                }
            }
            *CONN.lock().unwrap_or_else(|e| e.into_inner()) = None;
        })?;
    *CONN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Conn {
        stream,
        last: (String::new(), Instant::now()),
        window: false,
    });
    Ok(())
}

pub(crate) fn report(state: &str, progress: Option<f32>, message: &str) {
    let mut conn = CONN.lock().unwrap_or_else(|e| e.into_inner());
    let Some(c) = conn.as_mut() else { return };
    if progress.is_some() && c.last.0 == state && c.last.1.elapsed() < PROGRESS_EVERY {
        return;
    }
    let m = Message::new(
        "state",
        json!({ "state": state, "progress": progress, "message": message, "window": c.window }),
    );
    if protocol::write_frame(&mut c.stream, &m).is_err() {
        // half a frame may be out: the engine must see the link end, not wait for the rest
        let _ = c.stream.shutdown(std::net::Shutdown::Both);
        *conn = None;
        return;
    }
    c.last = (state.to_string(), Instant::now());
}

/// The launcher that started the game is still in front: take the focus from it.
pub(crate) fn window_shown(window: &winit::window::Window) {
    let state = {
        let mut conn = CONN.lock().unwrap_or_else(|e| e.into_inner());
        let Some(c) = conn.as_mut() else { return };
        c.window = true;
        if c.last.0.is_empty() { "starting".to_string() } else { c.last.0.clone() }
    };
    window.focus_window();
    report(&state, None, "");
}

/// A game this one starts in its place would report as this one.
pub(crate) fn unlinked(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd.env_remove(ENV_ADDR).env_remove(ENV_TOKEN).env_remove("OMSI_INSTANCE")
}

pub(crate) fn failed(message: &str) {
    let text: String = message.chars().take(2000).collect();
    report("failed", None, &text);
}
