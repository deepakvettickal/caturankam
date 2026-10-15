//! A Lichess TV game as the UI sees it. The feed sends the position after
//! every move, which is the source of truth; clocks are interpolated locally
//! between moves.

use caturankam_core::{Color, Game, MoveEvent, Outcome};
use caturankam_lichess::{TvFeatured, TvFen, TvPlayer};
use std::time::{Duration, Instant};

pub struct TvGame {
    /// The side Lichess shows at the bottom.
    pub bottom: Color,
    white: TvPlayer,
    black: TvPlayer,
    pub game: Game,
    /// Seconds left as of `synced`. The feed only sends them with a move, so
    /// they're unknown until the first move after tuning in.
    clocks: Option<(u32, u32)>,
    synced: Instant,
}

impl TvGame {
    pub fn new(f: TvFeatured) -> Result<Self, caturankam_core::Error> {
        let player = |c: &str| {
            f.players
                .iter()
                .find(|p| p.color == c)
                .cloned()
                .unwrap_or_default()
        };
        Ok(Self {
            bottom: if f.orientation == "black" {
                Color::Black
            } else {
                Color::White
            },
            white: player("white"),
            black: player("black"),
            game: Game::from_fen(&f.fen)?,
            clocks: None,
            synced: Instant::now(),
        })
    }

    /// Applies a position from the feed. Returns the move to animate when it
    /// follows from the position shown; otherwise jumps straight to it.
    pub fn update(&mut self, m: &TvFen) -> Result<Option<MoveEvent>, caturankam_core::Error> {
        let next = Game::from_fen(&m.fen)?;
        let mut played = self.game.clone();
        let ev =
            m.lm.as_deref()
                .and_then(|uci| played.play_uci(uci).ok())
                .filter(|_| played.board() == next.board() && played.turn() == next.turn());
        // The played copy also keeps the last-move highlight.
        self.game = if ev.is_some() { played } else { next };
        if let (Some(w), Some(b)) = (m.wc, m.bc) {
            self.clocks = Some((w, b));
        }
        self.synced = Instant::now();
        Ok(ev)
    }

    pub fn player(&self, c: Color) -> &TvPlayer {
        match c {
            Color::White => &self.white,
            Color::Black => &self.black,
        }
    }

    pub fn is_over(&self) -> bool {
        self.game.outcome() != Outcome::Unknown
    }

    /// Remaining time for `c` at `now`: the side to move loses time locally.
    pub fn clock(&self, c: Color, now: Instant) -> Option<Duration> {
        let (w, b) = self.clocks?;
        let left = Duration::from_secs(u64::from(if c == Color::White { w } else { b }));
        Some(if self.ticking() && self.game.turn() == c {
            left.saturating_sub(now.saturating_duration_since(self.synced))
        } else {
            left
        })
    }

    pub fn ticking(&self) -> bool {
        self.clocks.is_some() && !self.is_over()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recorded from the live feed.
    fn featured() -> TvGame {
        let f = r#"{"id":"oujEPn9h","orientation":"black","players":[{"color":"white","user":{"name":"noBrainNoChance"},"rating":2995,"seconds":60},{"color":"black","user":{"name":"indianstar","title":"GM"},"rating":3154,"seconds":60}],"fen":"2q1n1k1/1p4b1/3p4/p2P4/4p3/2P1Br2/P2Q4/2KR3R b - - 1 25"}"#;
        TvGame::new(serde_json::from_str(f).unwrap()).unwrap()
    }

    fn fen(json: &str) -> TvFen {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn follows_moves_and_clocks() {
        let mut g = featured();
        assert_eq!(g.bottom, Color::Black);
        assert_eq!(g.player(Color::Black).rating, Some(3154));
        assert_eq!(g.clock(Color::White, Instant::now()), None); // unknown until a move

        let ev = g
            .update(&fen(r#"{"fen":"2q1n1k1/1p6/3p4/p2P4/4p3/2b1Br2/P2Q4/2KR3R w - - 0 26","lm":"g7c3","wc":24,"bc":42}"#))
            .unwrap()
            .expect("a move to animate");
        assert!(ev.capture);
        assert_eq!(g.game.last_uci().as_deref(), Some("g7c3"));
        // White is to move, so only White's clock runs.
        let later = Instant::now() + Duration::from_secs(5);
        assert_eq!(g.clock(Color::Black, later), Some(Duration::from_secs(42)));
        assert!(g.clock(Color::White, later).unwrap() <= Duration::from_secs(19));
    }

    #[test]
    fn jumps_when_a_move_was_missed() {
        let mut g = featured();
        // Two moves later: not reachable by `lm` from the shown position.
        let ev = g
            .update(&fen(r#"{"fen":"2q1n1k1/1p6/3p4/p2P4/4p3/2b1Br2/P2Q4/2K3RR b - - 1 26","lm":"d1g1","wc":23,"bc":42}"#))
            .unwrap();
        assert!(ev.is_none());
        assert_eq!(g.game.turn(), Color::Black);
    }
}
