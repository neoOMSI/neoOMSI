use crate::{self as protocol, Message};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const ENV_ADDR: &str = "OMSI_CONTROL";
pub const ENV_TOKEN: &str = "OMSI_CONTROL_TOKEN";

/// Before the token is checked, any local program can connect.
const HELLO_MAX: usize = 4096;
const HELLO_WITHIN: Duration = Duration::from_secs(5);
const UNANSWERED_MAX: usize = 16;
static UNANSWERED: AtomicUsize = AtomicUsize::new(0);

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct GameState {
    pub state: String,
    #[serde(default)]
    pub progress: Option<f32>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub window: bool,
}

type Sink = Box<dyn Fn(&str) + Send + Sync>;

struct Game {
    conn: u64,
    stream: TcpStream,
    state: GameState,
}

struct Link {
    addr: SocketAddr,
    token: String,
    games: Mutex<HashMap<String, Game>>,
    changed: Sink,
}

static LINK: OnceLock<Link> = OnceLock::new();

fn token() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn valid_instance(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `changed` gets a game's instance id whenever it reports or hangs up.
pub fn listen(changed: impl Fn(&str) + Send + Sync + 'static) -> std::io::Result<SocketAddr> {
    if let Some(l) = LINK.get() {
        return Ok(l.addr);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let addr = listener.local_addr()?;
    let link = Link {
        addr,
        token: token()?,
        games: Mutex::new(HashMap::new()),
        changed: Box::new(changed),
    };
    if LINK.set(link).is_err() {
        return Ok(LINK.get().map(|l| l.addr).unwrap_or(addr));
    }
    std::thread::Builder::new()
        .name("game link".into())
        .spawn(move || {
            let mut next = 0u64;
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                };
                if UNANSWERED.load(Ordering::SeqCst) >= UNANSWERED_MAX {
                    continue;
                }
                UNANSWERED.fetch_add(1, Ordering::SeqCst);
                next += 1;
                let conn = next;
                let spawned = std::thread::Builder::new()
                    .name("game link conn".into())
                    .spawn(move || serve(stream, conn));
                if spawned.is_err() {
                    UNANSWERED.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })?;
    Ok(addr)
}

struct Within<'a>(&'a TcpStream, Instant);

impl Read for Within<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.1.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.0.set_read_timeout(Some(left))?;
        (&mut &*self.0).read(buf)
    }
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

fn serve(stream: TcpStream, conn: u64) {
    let hello = protocol::read_frame_max(&mut Within(&stream, Instant::now() + HELLO_WITHIN), HELLO_MAX);
    UNANSWERED.fetch_sub(1, Ordering::SeqCst);
    let Ok(Some(hello)) = hello else { return };
    if hello.kind == "hello" {
        welcome(stream, conn, hello);
    }
}

fn welcome(mut stream: TcpStream, conn: u64, hello: Message) {
    let Some(link) = LINK.get() else { return };
    let _ = stream.set_nodelay(true);
    let token = hello.payload.get("token").and_then(|v| v.as_str());
    let id = hello
        .payload
        .get("instance")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !token.is_some_and(|t| same(t, &link.token)) || !valid_instance(&id) {
        let _ = protocol::write_frame(
            &mut stream,
            &Message::new("refused", json!({ "reason": "unknown game" })),
        );
        return;
    }
    let Ok(writer) = stream.try_clone() else { return };
    let _ = stream.set_read_timeout(None);
    let _ = writer.set_write_timeout(Some(Duration::from_millis(500)));
    link.games.lock().unwrap_or_else(|e| e.into_inner()).insert(
        id.clone(),
        Game {
            conn,
            stream: writer,
            state: GameState {
                state: "starting".into(),
                ..Default::default()
            },
        },
    );
    let _ = protocol::write_frame(&mut stream, &Message::new("welcome", json!({})));
    (link.changed)(&id);
    while let Ok(Some(m)) = protocol::read_frame(&mut stream) {
        if m.kind != "state" {
            continue;
        }
        let Ok(state) = serde_json::from_value::<GameState>(m.payload) else {
            continue;
        };
        if let Some(g) = link
            .games
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&id)
            .filter(|g| g.conn == conn)
        {
            g.state = state;
        }
        (link.changed)(&id);
    }
    let mut games = link.games.lock().unwrap_or_else(|e| e.into_inner());
    if games.get(&id).map(|g| g.conn == conn).unwrap_or(false) {
        games.remove(&id);
    }
    drop(games);
    (link.changed)(&id);
}

pub fn env() -> Vec<(&'static str, String)> {
    LINK.get()
        .map(|l| vec![(ENV_ADDR, l.addr.to_string()), (ENV_TOKEN, l.token.clone())])
        .unwrap_or_default()
}

pub fn state(instance: &str) -> Option<GameState> {
    LINK.get()?
        .games
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(instance)
        .map(|g| g.state.clone())
}

/// False when the game is not connected: only a signal reaches it then.
pub fn request_quit(instance: &str) -> bool {
    let Some(link) = LINK.get() else { return false };
    let mut games = link.games.lock().unwrap_or_else(|e| e.into_inner());
    let Some(g) = games.get_mut(instance) else {
        return false;
    };
    protocol::write_frame(&mut g.stream, &Message::new("quit", json!({}))).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CHANGES: AtomicUsize = AtomicUsize::new(0);

    fn connect(token: &str, instance: &str) -> (TcpStream, Option<Message>) {
        let addr = listen(|_| {
            CHANGES.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        protocol::write_frame(
            &mut s,
            &Message::new("hello", json!({ "token": token, "instance": instance, "pid": 1 })),
        )
        .unwrap();
        let answer = protocol::read_frame(&mut s).ok().flatten();
        (s, answer)
    }

    fn wait_for(f: impl Fn() -> bool) -> bool {
        (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(20));
            f()
        })
    }

    #[test]
    fn tokens_are_256_random_bits() {
        let (a, b) = (token().unwrap(), token().unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn a_game_reports_its_state_and_is_asked_to_quit() {
        let (_, answer) = connect("wrong", "game-a");
        assert_eq!(answer.map(|m| m.kind).as_deref(), Some("refused"));
        assert_eq!(state("game-a"), None);
        let token = env().into_iter().find(|(k, _)| *k == ENV_TOKEN).unwrap().1;
        let (_, answer) = connect(&token, "../escape");
        assert_eq!(answer.map(|m| m.kind).as_deref(), Some("refused"));

        let (mut game, answer) = connect(&token, "game-b");
        assert_eq!(answer.map(|m| m.kind).as_deref(), Some("welcome"));
        assert_eq!(state("game-b").unwrap().state, "starting");
        protocol::write_frame(
            &mut game,
            &Message::new("state", json!({ "state": "loading", "progress": 0.5, "message": "tiles" })),
        )
        .unwrap();
        assert!(wait_for(|| state("game-b").and_then(|s| s.progress) == Some(0.5)));
        assert!(CHANGES.load(Ordering::SeqCst) >= 2);

        assert!(request_quit("game-b"));
        assert_eq!(protocol::read_frame(&mut game).unwrap().unwrap().kind, "quit");
        assert!(!request_quit("not-connected"));

        drop(game);
        assert!(wait_for(|| state("game-b").is_none()), "gone once it hangs up");
    }
}
