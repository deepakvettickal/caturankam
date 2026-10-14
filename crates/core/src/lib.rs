//! Game model on top of shakmaty. No I/O, no UI.

use shakmaty::{CastlingMode, Chess, Move, Position, fen::Fen};

pub use shakmaty::{Board, Color, File, KnownOutcome, Outcome, Piece, Rank, Role, Square};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid FEN: {0}")]
    Fen(String),
    #[error("illegal move")]
    IllegalMove,
}

/// What a move did, so the UI can animate it and play the right sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveEvent {
    pub from: Square,
    /// Where the moving piece lands (the king's square when castling).
    pub to: Square,
    pub piece: Piece,
    pub capture: bool,
    pub check: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Game {
    position: Chess,
    last_move: Option<(Square, Square)>,
}

impl Game {
    pub fn from_fen(fen: &str) -> Result<Self, Error> {
        let fen: Fen = fen.parse().map_err(|e| Error::Fen(format!("{e}")))?;
        let position = fen
            .into_position(CastlingMode::Standard)
            .map_err(|e| Error::Fen(e.to_string()))?;
        Ok(Self {
            position,
            last_move: None,
        })
    }

    pub fn board(&self) -> &Board {
        self.position.board()
    }

    pub fn turn(&self) -> Color {
        self.position.turn()
    }

    pub fn is_check(&self) -> bool {
        self.position.is_check()
    }

    /// Checkmate, stalemate or insufficient material. Repetition and the
    /// 50-move rule are left to the server.
    pub fn outcome(&self) -> Outcome {
        self.position.outcome()
    }

    pub fn last_move(&self) -> Option<(Square, Square)> {
        self.last_move
    }

    /// Squares the piece on `from` can legally move to.
    pub fn targets(&self, from: Square) -> Vec<Square> {
        let mut targets: Vec<Square> = self
            .position
            .legal_moves()
            .into_iter()
            .filter(|m| m.from() == Some(from))
            .map(landing)
            .collect();
        targets.dedup(); // the four promotion moves share a target
        targets
    }

    /// True if `from -> to` is legal only with a promotion piece chosen.
    pub fn is_promotion(&self, from: Square, to: Square) -> bool {
        self.find(from, to, None).is_none() && self.find(from, to, Some(Role::Queen)).is_some()
    }

    pub fn play(
        &mut self,
        from: Square,
        to: Square,
        promotion: Option<Role>,
    ) -> Result<MoveEvent, Error> {
        let m = self.find(from, to, promotion).ok_or(Error::IllegalMove)?;
        let piece = self.board().piece_at(from).ok_or(Error::IllegalMove)?;
        let to = landing(m);
        self.position.play_unchecked(m);
        self.last_move = Some((from, to));
        Ok(MoveEvent {
            from,
            to,
            piece,
            capture: m.is_capture(),
            check: self.position.is_check(),
        })
    }

    fn find(&self, from: Square, to: Square, promotion: Option<Role>) -> Option<Move> {
        // Castling matches either the king's landing square or the rook's
        // square (shakmaty's internal encoding), like Lichess's board.
        self.position.legal_moves().into_iter().find(|&m| {
            m.from() == Some(from)
                && (landing(m) == to || m.to() == to)
                && m.promotion() == promotion
        })
    }
}

/// Where the moving piece ends up. Differs from `Move::to` only for castling.
fn landing(m: Move) -> Square {
    m.to_uci(CastlingMode::Standard).to().unwrap_or(m.to())
}

#[cfg(test)]
mod tests {
    use super::*;
    use Square::*;

    #[test]
    fn opening_moves() {
        let mut g = Game::default();
        assert_eq!(g.targets(E2), vec![E3, E4]);
        assert_eq!(g.targets(E7), vec![]); // not black's turn
        let ev = g.play(E2, E4, None).unwrap();
        assert!(!ev.capture && !ev.check);
        assert_eq!(g.turn(), Color::Black);
        assert_eq!(g.play(E2, E4, None), Err(Error::IllegalMove));
    }

    #[test]
    fn castling_by_king_or_rook_square() {
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        let mut g = Game::from_fen(fen).unwrap();
        assert!(g.targets(E1).contains(&G1));
        assert_eq!(g.play(E1, G1, None).unwrap().to, G1);
        assert_eq!(g.board().piece_at(F1).map(|p| p.role), Some(Role::Rook));

        let mut g = Game::from_fen(fen).unwrap();
        assert_eq!(g.play(E1, A1, None).unwrap().to, C1);
    }

    #[test]
    fn promotion_needs_a_piece() {
        let mut g = Game::from_fen("8/P7/8/8/8/8/8/k6K w - - 0 1").unwrap();
        assert!(g.is_promotion(A7, A8));
        assert!(!g.is_promotion(H1, H2));
        assert_eq!(g.targets(A7), vec![A8]);
        assert_eq!(g.play(A7, A8, None), Err(Error::IllegalMove));
        g.play(A7, A8, Some(Role::Knight)).unwrap();
        assert_eq!(g.board().piece_at(A8).map(|p| p.role), Some(Role::Knight));
    }

    #[test]
    fn capture_check_and_mate() {
        // Fool's mate.
        let mut g = Game::default();
        for (f, t) in [(F2, F3), (E7, E5), (G2, G4)] {
            g.play(f, t, None).unwrap();
        }
        let ev = g.play(D8, H4, None).unwrap();
        assert!(ev.check && !ev.capture);
        assert_eq!(
            g.outcome(),
            Outcome::Known(KnownOutcome::Decisive {
                winner: Color::Black
            })
        );

        let mut g = Game::from_fen("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1").unwrap();
        assert!(g.play(E4, D5, None).unwrap().capture);
    }

    #[test]
    fn bad_fen() {
        assert!(matches!(Game::from_fen("nonsense"), Err(Error::Fen(_))));
    }
}
