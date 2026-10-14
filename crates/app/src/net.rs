//! The network thread. Started lazily on first need (login, a saved session,
//! or an asset download), so a logged-out user never pays for it.
//!
//! The UI sends `Cmd`s; this thread runs a single-threaded tokio runtime and
//! answers with `Evt`s, waking the UI with `request_repaint`. The egui thread
//! never blocks on the network.

use caturankam_lichess::{self as lichess, Client, Event, GameEvent, HttpClient, Side};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

const KEYRING_SERVICE: &str = "caturankam";
const KEYRING_USER: &str = "lichess-token";

pub enum Cmd {
    /// Resume a saved session: token from the keychain.
    Connect,
    /// Interactive browser login.
    Login,
    Logout,
    Seek {
        minutes: u32,
        increment: u32,
        rated: bool,
    },
    CancelSeek,
    ChallengeAi {
        level: u8,
        limit_secs: u32,
        increment: u32,
        color: Option<Side>,
    },
    Move {
        game: String,
        uci: String,
    },
    Resign(String),
    Abort(String),
    Draw {
        game: String,
        accept: bool,
    },
    /// Download a public asset and save it to `cache`.
    Fetch {
        url: String,
        cache: PathBuf,
    },
}

pub enum Evt {
    LoggedIn {
        username: String,
        /// Lichess's board and piece choice; only sent on a fresh login.
        prefs: Option<lichess::Prefs>,
    },
    LoggedOut {
        reason: Option<String>,
    },
    Notice(String),
    SeekEnded,
    Game(GameEvent),
    Fetched {
        url: String,
        bytes: Result<Vec<u8>, String>,
    },
}

pub struct Net {
    tx: UnboundedSender<Cmd>,
    pub rx: mpsc::Receiver<Evt>,
}

impl Net {
    pub fn start(ctx: egui::Context) -> Self {
        let (tx, cmds) = unbounded_channel();
        let (evt_tx, rx) = mpsc::channel();
        let out = Out { tx: evt_tx, ctx };
        std::thread::Builder::new()
            .name("net".into())
            .spawn(move || {
                // One thread is plenty: everything here waits on the network.
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                rt.block_on(run(cmds, out));
            })
            .expect("spawn net thread");
        Self { tx, rx }
    }

    pub fn send(&self, cmd: Cmd) {
        // Only fails if the net thread died; the UI keeps working offline.
        let _ = self.tx.send(cmd);
    }
}

/// Sends events to the UI and wakes it. Cheap to clone into tasks.
#[derive(Clone)]
struct Out {
    tx: mpsc::Sender<Evt>,
    ctx: egui::Context,
}

impl Out {
    fn send(&self, evt: Evt) {
        let _ = self.tx.send(evt);
        self.ctx.request_repaint();
    }

    fn notice(&self, msg: impl Into<String>) {
        self.send(Evt::Notice(msg.into()));
    }
}

/// A task that is cancelled when its handle is dropped, so tearing down a
/// parent (e.g. logout) tears down everything it started.
struct Owned(JoinHandle<()>);

impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// What a session task tells `run`: start with this token, or end.
type SessionMsg = Result<String, String>;

struct State {
    http: HttpClient,
    client: Option<Client>,
    events: Option<Owned>,
    seek: Option<Owned>,
    login: Option<Owned>,
    session: UnboundedSender<SessionMsg>,
    out: Out,
}

async fn run(mut cmds: UnboundedReceiver<Cmd>, out: Out) {
    let (session, mut session_rx) = unbounded_channel();
    let mut st = State {
        http: lichess::http(),
        client: None,
        events: None,
        seek: None,
        login: None,
        session,
        out,
    };
    loop {
        tokio::select! {
            Some(msg) = session_rx.recv() => st.session_changed(msg),
            cmd = cmds.recv() => match cmd {
                Some(cmd) => st.handle(cmd),
                None => return, // the UI is gone
            },
        }
    }
}

impl State {
    fn session_changed(&mut self, msg: SessionMsg) {
        self.seek = None;
        match msg {
            Ok(token) => {
                let c = Client::new(self.http.clone(), token);
                let task = tokio::spawn(event_loop(
                    c.clone(),
                    self.session.clone(),
                    self.out.clone(),
                ));
                self.events = Some(Owned(task));
                self.client = Some(c);
            }
            Err(reason) => {
                self.events = None;
                self.client = None;
                self.out.send(Evt::LoggedOut {
                    reason: Some(reason),
                });
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        let out = self.out.clone();
        match cmd {
            Cmd::Connect => {
                let (http, session) = (self.http.clone(), self.session.clone());
                tokio::spawn(async move {
                    match keyring_entry().and_then(|e| e.get_password().map_err(|e| e.to_string()))
                    {
                        Ok(token) => verify(http, token, false, session, out).await,
                        Err(_) => {
                            let _ = session.send(Err("Please log in again.".into()));
                        }
                    }
                });
            }
            Cmd::Login => {
                let (http, session) = (self.http.clone(), self.session.clone());
                // Replacing the handle cancels a login already in progress.
                self.login = Some(Owned(tokio::spawn(async move {
                    match lichess::auth::login(&http, open_browser).await {
                        Ok(token) => {
                            let saved = keyring_entry()
                                .and_then(|e| e.set_password(&token).map_err(|e| e.to_string()));
                            if let Err(e) = saved {
                                out.notice(format!("Couldn't save the login to the keychain: {e}"));
                            }
                            verify(http, token, true, session, out).await;
                        }
                        Err(e) => out.notice(e.to_string()),
                    }
                })));
            }
            Cmd::Logout => {
                self.events = None;
                self.seek = None;
                if let Some(c) = self.client.take() {
                    tokio::spawn(async move {
                        let _ = c.revoke().await; // best effort; the local token is gone either way
                    });
                }
                if let Ok(e) = keyring_entry() {
                    let _ = e.delete_credential();
                }
                out.send(Evt::LoggedOut { reason: None });
            }
            Cmd::Fetch { url, cache } => {
                let http = self.http.clone();
                tokio::spawn(async move {
                    let bytes = lichess::fetch_asset(&http, &url)
                        .await
                        .map_err(|e| e.to_string());
                    if let (Ok(b), Some(dir)) = (&bytes, cache.parent()) {
                        let _ =
                            std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&cache, b));
                    }
                    out.send(Evt::Fetched { url, bytes });
                });
            }
            cmd => {
                let Some(c) = self.client.clone() else {
                    out.notice("Log in to play online.");
                    return;
                };
                self.game_cmd(cmd, c, out);
            }
        }
    }

    fn game_cmd(&mut self, cmd: Cmd, c: Client, out: Out) {
        match cmd {
            Cmd::Seek {
                minutes,
                increment,
                rated,
            } => {
                self.seek = Some(Owned(tokio::spawn(async move {
                    if let Err(e) = c.seek(minutes, increment, rated).await {
                        out.notice(e.to_string());
                    }
                    out.send(Evt::SeekEnded);
                })));
            }
            // Dropping the request closes the connection, which cancels the seek.
            Cmd::CancelSeek => self.seek = None,
            Cmd::ChallengeAi {
                level,
                limit_secs,
                increment,
                color,
            } => report(out, async move {
                // The game itself arrives through the event stream.
                c.challenge_ai(level, limit_secs, increment, color)
                    .await
                    .map(drop)
            }),
            Cmd::Move { game, uci } => report(out, async move { c.make_move(&game, &uci).await }),
            Cmd::Resign(game) => report(out, async move { c.resign(&game).await }),
            Cmd::Abort(game) => report(out, async move { c.abort(&game).await }),
            Cmd::Draw { game, accept } => report(out, async move { c.draw(&game, accept).await }),
            Cmd::Connect | Cmd::Login | Cmd::Logout | Cmd::Fetch { .. } => {
                unreachable!("handled in State::handle")
            }
        }
    }
}

/// Runs a request in the background; any error becomes a notice.
fn report(out: Out, req: impl Future<Output = Result<(), lichess::Error>> + Send + 'static) {
    tokio::spawn(async move {
        if let Err(e) = req.await {
            out.notice(e.to_string());
        }
    });
}

/// Checks the token with `/api/account`, then starts the session.
async fn verify(
    http: HttpClient,
    token: String,
    fresh: bool,
    session: UnboundedSender<SessionMsg>,
    out: Out,
) {
    let c = Client::new(http, token.clone());
    match c.account().await {
        Ok(account) => {
            // Adopt Lichess's board and pieces only on a fresh login, so local choices stick.
            let prefs = if fresh {
                c.preferences().await.ok()
            } else {
                None
            };
            out.send(Evt::LoggedIn {
                username: account.username,
                prefs,
            });
            let _ = session.send(Ok(token));
        }
        Err(e) if e.is_unauthorized() => {
            if let Ok(entry) = keyring_entry() {
                let _ = entry.delete_credential();
            }
            let _ = session.send(Err(
                "Your Lichess session expired. Please log in again.".into()
            ));
        }
        Err(e) => out.notice(format!("Can't reach Lichess: {e}")),
    }
}

/// Keeps the event stream open while logged in, reconnecting with backoff.
/// Games that start (or are already running when we connect) get a game stream.
async fn event_loop(c: Client, session: UnboundedSender<SessionMsg>, out: Out) {
    let mut game: Option<(String, Owned)> = None;
    let mut backoff = Backoff::default();
    loop {
        let result = c
            .stream_events(|e| {
                backoff.reset();
                let Event::GameStart { game: info } = e else {
                    return;
                };
                let running = game
                    .as_ref()
                    .is_some_and(|(id, t)| *id == info.game_id && !t.0.is_finished());
                if !info.board_compatible() || running {
                    return;
                }
                let task = tokio::spawn(game_loop(c.clone(), info.game_id.clone(), out.clone()));
                game = Some((info.game_id, Owned(task)));
            })
            .await;
        if let Err(e) = &result
            && e.is_unauthorized()
        {
            let _ = session.send(Err(
                "Your Lichess session expired. Please log in again.".into()
            ));
            return;
        }
        backoff.wait().await;
    }
}

/// Streams one game until it ends, reconnecting if the connection drops.
async fn game_loop(c: Client, id: String, out: Out) {
    let mut backoff = Backoff::default();
    loop {
        let mut over = false;
        let result = c
            .stream_game(&id, |ev| {
                backoff.reset();
                let status = match &ev {
                    GameEvent::GameFull(f) => Some(&f.state.status),
                    GameEvent::GameState(s) => Some(&s.status),
                    _ => None,
                };
                if let Some(s) = status {
                    over = !matches!(s.as_str(), "created" | "started");
                }
                out.send(Evt::Game(ev));
            })
            .await;
        match result {
            _ if over => return,
            Err(lichess::Error::Api {
                status: 401 | 404, ..
            }) => return,
            _ => backoff.wait().await,
        }
    }
}

#[derive(Default)]
struct Backoff(u32);

impl Backoff {
    fn reset(&mut self) {
        self.0 = 0;
    }

    async fn wait(&mut self) {
        let secs = 1u64 << self.0.min(4); // 1, 2, 4, 8, then 16 s
        self.0 += 1;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let child = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let child = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let child = std::process::Command::new("xdg-open").arg(url).spawn();
    drop(child);
}
