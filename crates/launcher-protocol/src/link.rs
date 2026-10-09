use crate::api::{FromGame, GameHello, GameLink, GameLinkState, ToGame, from_game, to_game};
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

type Sink = Box<dyn Fn(&str) + Send + Sync>;

struct Game {
    conn: u64,
    stream: TcpStream,
    state: GameLink,
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

fn to_game(body: to_game::Body) -> ToGame {
    ToGame { body: Some(body) }
}

fn serve(stream: TcpStream, conn: u64) {
    let hello = crate::read_frame_max::<FromGame>(
        &mut Within(&stream, Instant::now() + HELLO_WITHIN),
        HELLO_MAX,
    );
    UNANSWERED.fetch_sub(1, Ordering::SeqCst);
    if let Ok(Some(FromGame {
        body: Some(from_game::Body::Hello(hello)),
    })) = hello
    {
        welcome(stream, conn, hello);
    }
}

fn welcome(mut stream: TcpStream, conn: u64, hello: GameHello) {
    let Some(link) = LINK.get() else { return };
    let _ = stream.set_nodelay(true);
    let id = hello.instance;
    if !same(&hello.token, &link.token) || !valid_instance(&id) {
        let refused = to_game(to_game::Body::Refused("unknown game".into()));
        let _ = crate::write_frame(&mut stream, &refused);
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
            state: GameLink {
                state: GameLinkState::Starting.into(),
                ..Default::default()
            },
        },
    );
    let _ = crate::write_frame(&mut stream, &to_game(to_game::Body::Welcome(Default::default())));
    (link.changed)(&id);
    while let Ok(Some(m)) = crate::read_frame::<FromGame>(&mut stream) {
        let Some(from_game::Body::State(state)) = m.body else {
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

pub fn state(instance: &str) -> Option<GameLink> {
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
    crate::write_frame(&mut g.stream, &to_game(to_game::Body::Quit(Default::default()))).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CHANGES: AtomicUsize = AtomicUsize::new(0);

    fn connect(token: &str, instance: &str) -> (TcpStream, Option<to_game::Body>) {
        let addr = listen(|_| {
            CHANGES.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let hello = GameHello {
            token: token.into(),
            instance: instance.into(),
            pid: 1,
            ..Default::default()
        };
        let hello = FromGame {
            body: Some(from_game::Body::Hello(hello)),
        };
        crate::write_frame(&mut s, &hello).unwrap();
        let answer = crate::read_frame::<ToGame>(&mut s).ok().flatten();
        (s, answer.and_then(|m| m.body))
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
        assert!(matches!(answer, Some(to_game::Body::Refused(_))));
        assert_eq!(state("game-a"), None);
        let token = env().into_iter().find(|(k, _)| *k == ENV_TOKEN).unwrap().1;
        let (_, answer) = connect(&token, "../escape");
        assert!(matches!(answer, Some(to_game::Body::Refused(_))));

        let (mut game, answer) = connect(&token, "game-b");
        assert!(matches!(answer, Some(to_game::Body::Welcome(_))));
        assert_eq!(state("game-b").unwrap().state(), GameLinkState::Starting);
        let loading = GameLink {
            state: GameLinkState::Loading.into(),
            progress: Some(0.5),
            message: "tiles".into(),
            window: false,
        };
        let loading = FromGame {
            body: Some(from_game::Body::State(loading)),
        };
        crate::write_frame(&mut game, &loading).unwrap();
        assert!(wait_for(|| state("game-b").and_then(|s| s.progress) == Some(0.5)));
        assert!(CHANGES.load(Ordering::SeqCst) >= 2);

        assert!(request_quit("game-b"));
        let quit = crate::read_frame::<ToGame>(&mut game).unwrap().unwrap();
        assert!(matches!(quit.body, Some(to_game::Body::Quit(_))));
        assert!(!request_quit("not-connected"));

        drop(game);
        assert!(wait_for(|| state("game-b").is_none()), "gone once it hangs up");
    }
}
