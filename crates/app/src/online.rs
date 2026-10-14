//! A live Lichess game as the UI sees it. The server is the source of truth:
//! every `gameState` rebuilds the position, and clocks are interpolated
//! locally only between server updates.

use caturankam_core::{Color, Game, MoveEvent};
use caturankam_lichess::{GameFull, GameState, Player};
use std::time::{Duration, Instant};

pub struct OnlineGame {
    pub id: String,
    pub me: Color,
    pub white: Player,
    pub black: Player,
    pub rated: bool,
    pub speed: String,
    initial_fen: String,
    pub game: Game,
    /// Clocks in ms as of `synced`. `None` for untimed games.
    clocks: Option<(u64, u64)>,
    synced: Instant,
    pub status: String,
    pub winner: Option<Color>,
    pub opponent_offers_draw: bool,
    /// Seconds until we may claim victory, when the opponent left.
    pub opponent_gone: Option<u32>,
}

impl OnlineGame {
    /// `my_id` is the logged-in user's id (lowercase username).
    pub fn new(full: GameFull, my_id: &str) -> Result<Self, caturankam_core::Error> {
        let me = if full.white.id.as_deref() == Some(my_id) {
            Color::White
        } else {
            Color::Black
        };
        let mut g = Self {
            id: full.id,
            me,
            white: full.white,
            black: full.black,
            rated: full.rated,
            speed: full.speed,
            initial_fen: full.initial_fen,
            game: Game::default(),
            clocks: full.clock.map(|_| (0, 0)),
            synced: Instant::now(),
            status: String::new(),
            winner: None,
            opponent_offers_draw: false,
            opponent_gone: None,
        };
        g.update(&full.state)?;
        Ok(g)
    }

    /// Applies a server state. Returns the opponent's new move, if there is
    /// one to animate; our own moves were already shown when we made them.
    pub fn update(&mut self, st: &GameState) -> Result<Option<MoveEvent>, caturankam_core::Error> {
        let (game, last) = Game::from_uci(&self.initial_fen, &st.moves)?;
        let opponent_moved = game.ply() == self.game.ply() + 1 && game.board() != self.game.board();
        self.game = game;
        if self.clocks.is_some() {
            self.clocks = Some((st.wtime, st.btime));
        }
        self.synced = Instant::now();
        self.status.clone_from(&st.status);
        self.winner = st.winner.as_deref().map(|w| {
            if w == "white" {
                Color::White
            } else {
                Color::Black
            }
        });
        self.opponent_offers_draw = match self.me {
            Color::White => st.bdraw,
            Color::Black => st.wdraw,
        };
        Ok(last.filter(|_| opponent_moved))
    }

    pub fn is_over(&self) -> bool {
        !matches!(self.status.as_str(), "created" | "started")
    }

    pub fn my_turn(&self) -> bool {
        !self.is_over() && self.game.turn() == self.me
    }

    /// Both sides have moved, so draw offers and resigning apply instead of aborting.
    pub fn can_abort(&self) -> bool {
        !self.is_over() && self.game.ply() < 2
    }

    pub fn player(&self, c: Color) -> &Player {
        match c {
            Color::White => &self.white,
            Color::Black => &self.black,
        }
    }

    /// Remaining time for `c` at `now`. The side to move loses time locally
    /// once both players have made their first move, as on Lichess.
    pub fn clock(&self, c: Color, now: Instant) -> Option<Duration> {
        let (w, b) = self.clocks?;
        let ms = if c == Color::White { w } else { b };
        let mut left = Duration::from_millis(ms);
        if self.ticking() && self.game.turn() == c {
            left = left.saturating_sub(now.saturating_duration_since(self.synced));
        }
        Some(left)
    }

    pub fn ticking(&self) -> bool {
        self.clocks.is_some() && !self.is_over() && self.game.ply() >= 2
    }

    pub fn result(&self) -> String {
        let side = |c: Color| if c == Color::White { "White" } else { "Black" };
        let how = match self.status.as_str() {
            "mate" => "Checkmate",
            "resign" => "Resignation",
            "stalemate" => "Stalemate",
            "timeout" => "Opponent left",
            "outoftime" => "Time out",
            "draw" => "Draw agreed",
            "aborted" => "Game aborted",
            "noStart" => "Game didn't start",
            "insufficientMaterialClaim" => "Insufficient material",
            _ => "Game over",
        };
        match self.winner {
            Some(w) => format!("{how} · {} wins", side(w)),
            None => how.into(),
        }
    }
}

/// `m:ss`, or `s.t` in the last ten seconds, like Lichess.
pub fn format_clock(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 10_000 {
        format!("{}.{}", ms / 1000, (ms % 1000) / 100)
    } else {
        let s = ms / 1000;
        if s >= 3600 {
            format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
        } else {
            format!("{}:{:02}", s / 60, s % 60)
        }
    }
}

/// When the clock display next changes, so the UI repaints only then
/// instead of every frame.
pub fn next_tick(d: Duration) -> Duration {
    let ms = d.as_millis() as u64;
    let step = if ms < 10_000 { 100 } else { 1000 };
    Duration::from_millis(ms % step + 1)
}

pub fn player_label(p: &Player) -> String {
    if let Some(level) = p.ai_level {
        return format!("Stockfish level {level}");
    }
    let name = p.name.as_deref().unwrap_or("Anonymous");
    let name = match &p.title {
        Some(t) => format!("{t} {name}"),
        None => name.to_string(),
    };
    match p.rating {
        Some(r) if p.provisional == Some(true) => format!("{name} ({r}?)"),
        Some(r) => format!("{name} ({r})"),
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full(moves: &str) -> GameFull {
        let json = format!(
            r#"{{"id":"g1","white":{{"id":"me","name":"Me","rating":1500}},"black":{{"aiLevel":3}},
            "initialFen":"startpos","clock":{{"initial":300000,"increment":0}},"rated":false,
            "speed":"blitz","state":{{"moves":"{moves}","wtime":300000,"btime":290000,
            "status":"started"}}}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    fn state(moves: &str, status: &str) -> GameState {
        serde_json::from_str(&format!(
            r#"{{"moves":"{moves}","wtime":1000,"btime":2000,"status":"{status}","bdraw":true}}"#
        ))
        .unwrap()
    }

    #[test]
    fn follows_the_server() {
        let mut g = OnlineGame::new(full(""), "me").unwrap();
        assert_eq!(g.me, Color::White);
        assert!(g.my_turn() && g.can_abort() && !g.ticking());

        // Our optimistic move, then the server echo: nothing to animate.
        g.game.play_uci("e2e4").unwrap();
        assert!(g.update(&state("e2e4", "started")).unwrap().is_none());

        // Opponent replies: animate it.
        let ev = g.update(&state("e2e4 e7e5", "started")).unwrap();
        assert_eq!(ev.map(|e| e.to.to_string()), Some("e5".into()));
        assert!(g.ticking() && g.opponent_offers_draw && !g.can_abort());

        // A rebuild from scratch (e.g. reconnect) doesn't animate.
        let mut fresh = OnlineGame::new(full("e2e4 e7e5"), "me").unwrap();
        assert!(
            fresh
                .update(&state("e2e4 e7e5", "started"))
                .unwrap()
                .is_none()
        );

        g.update(&state("e2e4 e7e5", "resign")).unwrap();
        assert!(g.is_over() && !g.my_turn());
        assert_eq!(g.result(), "Resignation");
        assert_eq!(player_label(&g.black), "Stockfish level 3");
        assert_eq!(player_label(&g.white), "Me (1500)");
    }

    #[test]
    fn clocks() {
        assert_eq!(format_clock(Duration::from_millis(185_000)), "3:05");
        assert_eq!(format_clock(Duration::from_millis(9_870)), "9.8");
        assert_eq!(format_clock(Duration::from_secs(5400)), "1:30:00");
        assert_eq!(
            next_tick(Duration::from_millis(185_250)),
            Duration::from_millis(251)
        );
        assert_eq!(
            next_tick(Duration::from_millis(9_870)),
            Duration::from_millis(71)
        );

        let g = OnlineGame::new(full(""), "me").unwrap();
        let later = Instant::now() + Duration::from_secs(5);
        // Not ticking before both players moved.
        assert_eq!(g.clock(Color::White, later), Some(Duration::from_secs(300)));
        assert_eq!(g.clock(Color::Black, later), Some(Duration::from_secs(290)));
    }
}
