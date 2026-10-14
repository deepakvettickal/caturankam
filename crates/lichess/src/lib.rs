//! Thin Lichess client for the endpoints Caturankam uses: OAuth login, account,
//! preferences, the event and Board game streams, seeks, AI challenges and moves.
//! Pure async I/O: no UI, no threads of its own.

pub mod auth;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

pub const LICHESS: &str = "https://lichess.org";
/// Static assets (piece sets, boards). Same files lichess.org loads in a browser.
pub const ASSETS: &str = "https://lichess1.org/assets";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Lichess returned {status}: {body}")]
    Api { status: u16, body: String },
    #[error("login failed: {0}")]
    Auth(String),
}

impl Error {
    /// The token was revoked or expired: the user has to log in again.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Error::Api { status: 401, .. })
    }
}

/// One shared HTTP client. `read_timeout` doubles as a dead-stream detector:
/// Lichess sends a keep-alive line every few seconds on every stream.
pub fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("caturankam/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .build()
        .expect("static client config is valid")
}

#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub id: String,
    pub username: String,
}

/// The parts of the user's Lichess preferences we mirror.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub piece_set: Option<String>,
    /// Board theme name, e.g. `brown`, `wood4`.
    pub theme: Option<String>,
}

/// `/api/stream/event`. Only game events matter to us; challenges come later.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    GameStart {
        game: GameInfo,
    },
    GameFinish {
        game: GameInfo,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub game_id: String,
    pub compat: Option<Compat>,
}

impl GameInfo {
    /// Games we can't play through the Board API (e.g. bullet) are skipped.
    pub fn board_compatible(&self) -> bool {
        self.compat.as_ref().is_none_or(|c| c.board)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Compat {
    pub board: bool,
}

/// `/api/board/game/stream/{id}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GameEvent {
    GameFull(Box<GameFull>),
    GameState(GameState),
    OpponentGone {
        gone: bool,
        #[serde(rename = "claimWinInSeconds")]
        claim_win_in_seconds: Option<u32>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameFull {
    pub id: String,
    pub white: Player,
    pub black: Player,
    /// `startpos` or a FEN.
    pub initial_fen: String,
    pub clock: Option<Clock>,
    pub rated: bool,
    pub speed: String,
    pub state: GameState,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub id: Option<String>,
    pub name: Option<String>,
    pub title: Option<String>,
    pub rating: Option<u32>,
    pub provisional: Option<bool>,
    pub ai_level: Option<u8>,
}

/// Milliseconds.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Clock {
    pub initial: u64,
    pub increment: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GameState {
    /// Space-separated UCI moves. Castling may be king-to-rook (`e1h1`).
    pub moves: String,
    pub wtime: u64,
    pub btime: u64,
    pub status: String,
    pub winner: Option<String>,
    #[serde(default)]
    pub wdraw: bool,
    #[serde(default)]
    pub bdraw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    White,
    Black,
}

impl Side {
    fn as_str(self) -> &'static str {
        match self {
            Side::White => "white",
            Side::Black => "black",
        }
    }
}

/// An authenticated client. Cheap to clone (the HTTP client is reference-counted).
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    token: String,
}

impl Client {
    pub fn new(http: reqwest::Client, token: String) -> Self {
        Self { http, token }
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("{LICHESS}{path}"))
            .bearer_auth(&self.token)
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{LICHESS}{path}"))
            .bearer_auth(&self.token)
    }

    pub async fn account(&self) -> Result<Account, Error> {
        json(self.get("/api/account")).await
    }

    pub async fn preferences(&self) -> Result<Prefs, Error> {
        #[derive(Deserialize)]
        struct Resp {
            prefs: Prefs,
        }
        Ok(json::<Resp>(self.get("/api/account/preferences"))
            .await?
            .prefs)
    }

    /// Runs until the stream ends or fails. Lichess allows one event stream per
    /// token; opening a new one closes the old.
    pub async fn stream_events(&self, on: impl FnMut(Event)) -> Result<(), Error> {
        ndjson(send(self.get("/api/stream/event")).await?, on).await
    }

    /// Runs until the game ends (Lichess closes the stream) or the connection fails.
    pub async fn stream_game(&self, id: &str, on: impl FnMut(GameEvent)) -> Result<(), Error> {
        let resp = send(self.get(&format!("/api/board/game/stream/{id}"))).await?;
        ndjson(resp, on).await
    }

    /// A real-time seek. Resolves when the seek is accepted or expires; dropping
    /// the future cancels the seek. The game itself arrives on the event stream.
    pub async fn seek(&self, minutes: u32, increment: u32, rated: bool) -> Result<(), Error> {
        let form = [
            ("time", minutes.to_string()),
            ("increment", increment.to_string()),
            ("rated", rated.to_string()),
        ];
        ndjson(
            send(self.post("/api/board/seek").form(&form)).await?,
            |_: serde_json::Value| (),
        )
        .await
    }

    /// Starts a game against Lichess's Stockfish (`level` 1..=8). Returns the game id.
    pub async fn challenge_ai(
        &self,
        level: u8,
        limit_secs: u32,
        increment: u32,
        color: Option<Side>,
    ) -> Result<String, Error> {
        #[derive(Deserialize)]
        struct Resp {
            id: String,
        }
        let mut form = vec![
            ("level", level.to_string()),
            ("clock.limit", limit_secs.to_string()),
            ("clock.increment", increment.to_string()),
        ];
        if let Some(c) = color {
            form.push(("color", c.as_str().into()));
        }
        Ok(json::<Resp>(self.post("/api/challenge/ai").form(&form))
            .await?
            .id)
    }

    pub async fn make_move(&self, game: &str, uci: &str) -> Result<(), Error> {
        send(self.post(&format!("/api/board/game/{game}/move/{uci}"))).await?;
        Ok(())
    }

    pub async fn resign(&self, game: &str) -> Result<(), Error> {
        send(self.post(&format!("/api/board/game/{game}/resign"))).await?;
        Ok(())
    }

    pub async fn abort(&self, game: &str) -> Result<(), Error> {
        send(self.post(&format!("/api/board/game/{game}/abort"))).await?;
        Ok(())
    }

    /// `true` offers or accepts a draw; `false` declines one.
    pub async fn draw(&self, game: &str, accept: bool) -> Result<(), Error> {
        let answer = if accept { "yes" } else { "no" };
        send(self.post(&format!("/api/board/game/{game}/draw/{answer}"))).await?;
        Ok(())
    }

    /// Revokes the token on Lichess (logout).
    pub async fn revoke(&self) -> Result<(), Error> {
        let req = self
            .http
            .delete(format!("{LICHESS}/api/token"))
            .bearer_auth(&self.token);
        send(req).await?;
        Ok(())
    }
}

/// Downloads a public asset (no auth).
pub async fn fetch_asset(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, Error> {
    Ok(send(http.get(url)).await?.bytes().await?.to_vec())
}

async fn send(req: reqwest::RequestBuilder) -> Result<reqwest::Response, Error> {
    let resp = req.send().await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let mut body = resp.text().await.unwrap_or_default();
    body.truncate(300);
    Err(Error::Api {
        status: status.as_u16(),
        body,
    })
}

async fn json<T: DeserializeOwned>(req: reqwest::RequestBuilder) -> Result<T, Error> {
    Ok(serde_json::from_slice(&send(req).await?.bytes().await?)?)
}

/// Feeds each JSON line of a streamed response to `on`, skipping keep-alive
/// blank lines and lines we can't parse (newer Lichess fields or event types).
async fn ndjson<T: DeserializeOwned>(
    mut resp: reqwest::Response,
    mut on: impl FnMut(T),
) -> Result<(), Error> {
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        buf.extend_from_slice(&chunk);
        for line in take_lines(&mut buf) {
            if let Ok(item) = serde_json::from_slice(&line) {
                on(item);
            }
        }
    }
    Ok(())
}

/// Removes complete, non-blank lines from the front of `buf`, leaving any
/// partial line for the next chunk.
fn take_lines(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    while let Some(i) = buf.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = buf.drain(..=i).collect();
        if !line.trim_ascii().is_empty() {
            lines.push(line);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_across_chunks() {
        let mut buf = b"{\"a\":1}\n\n{\"b\"".to_vec();
        assert_eq!(take_lines(&mut buf), vec![b"{\"a\":1}\n".to_vec()]);
        assert_eq!(buf, b"{\"b\"");
        buf.extend_from_slice(b":2}\n");
        assert_eq!(take_lines(&mut buf).len(), 1);
        assert!(buf.is_empty());
    }

    // Payloads from the Lichess API docs examples.
    #[test]
    fn parses_game_stream() {
        let full = r#"{"id":"xcoxhDvh","variant":{"key":"standard","name":"Standard","short":"Std"},"speed":"blitz","perf":{"name":"Blitz"},"rated":false,"createdAt":1789845859939,"white":{"id":"kenneth","name":"Kenneth","title":null,"rating":804},"black":{"id":"salma","name":"Salma","title":null,"rating":714},"initialFen":"startpos","clock":{"initial":300000,"increment":0},"type":"gameFull","state":{"type":"gameState","moves":"","wtime":300000,"btime":300000,"winc":0,"binc":0,"status":"started"}}"#;
        let GameEvent::GameFull(g) = serde_json::from_str(full).unwrap() else {
            panic!("expected gameFull")
        };
        assert_eq!(g.white.name.as_deref(), Some("Kenneth"));
        assert_eq!(g.clock.unwrap().initial, 300_000);
        assert_eq!(g.state.status, "started");

        let state = r#"{"type":"gameState","moves":"e2e4","wtime":300000,"btime":300000,"winc":0,"binc":0,"status":"started"}"#;
        assert!(
            matches!(serde_json::from_str(state), Ok(GameEvent::GameState(s)) if s.moves == "e2e4" && !s.wdraw)
        );

        let gone = r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":50}"#;
        assert!(matches!(
            serde_json::from_str(gone),
            Ok(GameEvent::OpponentGone {
                gone: true,
                claim_win_in_seconds: Some(50)
            })
        ));

        let chat = r#"{"type":"chatLine","room":"player","username":"x","text":"hi"}"#;
        assert!(matches!(serde_json::from_str(chat), Ok(GameEvent::Other)));
    }

    #[test]
    fn parses_events() {
        let ai = r#"{"type":"gameStart","game":{"fullId":"YfjTIV43miXK","gameId":"YfjTIV43","color":"white","opponent":{"id":null,"username":"Stockfish level 1","ai":1},"compat":{"bot":false,"board":true},"id":"YfjTIV43"}}"#;
        let Event::GameStart { game } = serde_json::from_str(ai).unwrap() else {
            panic!("expected gameStart")
        };
        assert_eq!(game.game_id, "YfjTIV43");
        assert!(game.board_compatible());

        let challenge = r#"{"type":"challenge","challenge":{"id":"x"}}"#;
        assert!(matches!(serde_json::from_str(challenge), Ok(Event::Other)));
    }
}
