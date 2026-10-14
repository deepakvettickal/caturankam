//! Game model on top of shakmaty. No I/O, no UI.

use shakmaty::{
    Bitboard, CastlingMode, CastlingSide, Chess, Move, Position, attacks, fen::Fen, san::SanPlus,
    uci::UciMove,
};

pub use shakmaty::{Board, Color, File, KnownOutcome, Outcome, Piece, Rank, Role, Square};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid FEN: {0}")]
    Fen(String),
    #[error("illegal move")]
    IllegalMove,
    #[error("invalid UCI move: {0}")]
    Uci(String),
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
    /// Where the game started, so earlier positions can be rebuilt.
    initial: Chess,
    /// Every move played, in order.
    moves: Vec<Move>,
    position: Chess,
    last_move: Option<(Square, Square)>,
    /// Moves played so far, in SAN (e.g. `e4`, `Nxf7+`, `O-O`).
    san: Vec<String>,
    last_uci: Option<String>,
}

impl Game {
    pub fn from_fen(fen: &str) -> Result<Self, Error> {
        let fen: Fen = fen.parse().map_err(|e| Error::Fen(format!("{e}")))?;
        let position: Chess = fen
            .into_position(CastlingMode::Standard)
            .map_err(|e| Error::Fen(e.to_string()))?;
        Ok(Self {
            initial: position.clone(),
            position,
            ..Self::default()
        })
    }

    /// Rebuilds a game the way Lichess streams it: a start position
    /// (`startpos` or a FEN) plus space-separated UCI moves. Also returns the
    /// last move's event, so the UI can animate it.
    pub fn from_uci(initial: &str, moves: &str) -> Result<(Self, Option<MoveEvent>), Error> {
        let mut game = match initial {
            "startpos" | "" => Self::default(),
            fen => Self::from_fen(fen)?,
        };
        let mut last = None;
        for uci in moves.split_whitespace() {
            last = Some(game.play_uci(uci)?);
        }
        Ok((game, last))
    }

    /// Accepts both castling forms: king two squares (`e1g1`) or king to rook (`e1h1`).
    pub fn play_uci(&mut self, uci: &str) -> Result<MoveEvent, Error> {
        let parsed: UciMove = uci.parse().map_err(|_| Error::Uci(uci.into()))?;
        match parsed {
            UciMove::Normal {
                from,
                to,
                promotion,
            } => self.play(from, to, promotion),
            _ => Err(Error::Uci(uci.into())),
        }
    }

    /// The last move in UCI with standard castling (`e1g1`), as Lichess expects it.
    pub fn last_uci(&self) -> Option<String> {
        self.last_uci.clone()
    }

    pub fn ply(&self) -> usize {
        self.san.len()
    }

    /// The game as it stood after `ply` moves (clamped to the moves played),
    /// for stepping back through history.
    pub fn at(&self, ply: usize) -> Game {
        let mut game = Game {
            initial: self.initial.clone(),
            position: self.initial.clone(),
            ..Game::default()
        };
        for &m in self.moves.iter().take(ply) {
            game.apply(m);
        }
        game
    }

    /// Where the piece on `from` could go on its owner's next turn, for a
    /// premove: its movement pattern, ignoring other pieces (they may move),
    /// except that it can't land on its own side's pieces. A premove is
    /// checked for real legality only when it's played.
    pub fn premove_targets(&self, from: Square) -> Vec<Square> {
        let board = self.board();
        let Some(piece) = board.piece_at(from) else {
            return Vec::new();
        };
        let mut squares = match piece.role {
            Role::Pawn => {
                let forward = if piece.color == Color::White { 8 } else { -8 };
                let mut push = Bitboard::EMPTY;
                if let Some(one) = from.offset(forward) {
                    push.add(one);
                    if from.rank() == piece.color.relative_rank(Rank::Second)
                        && let Some(two) = one.offset(forward)
                    {
                        push.add(two);
                    }
                }
                push | attacks::pawn_attacks(piece.color, from)
            }
            Role::King => {
                let mut squares = attacks::king_attacks(from);
                let castles = self.position.castles();
                let home = Square::from_coords(File::E, piece.color.backrank());
                if from == home {
                    for (side, file) in [
                        (CastlingSide::KingSide, File::G),
                        (CastlingSide::QueenSide, File::C),
                    ] {
                        if castles.has(piece.color, side) {
                            squares.add(Square::from_coords(file, piece.color.backrank()));
                        }
                    }
                }
                squares
            }
            _ => attacks::attacks(from, piece, Bitboard::EMPTY),
        };
        squares &= !board.by_color(piece.color);
        squares.into_iter().collect()
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

    pub fn san_moves(&self) -> &[String] {
        &self.san
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
        self.board().piece_at(from).ok_or(Error::IllegalMove)?;
        Ok(self.apply(m))
    }

    /// Plays a move already known to be legal.
    fn apply(&mut self, m: Move) -> MoveEvent {
        let from = m.from().expect("standard chess moves have an origin");
        let piece = self
            .board()
            .piece_at(from)
            .expect("a legal move moves a piece");
        let to = landing(m);
        self.last_uci = Some(m.to_uci(CastlingMode::Standard).to_string());
        let san = SanPlus::from_move_and_play_unchecked(&mut self.position, m);
        self.san.push(san.to_string());
        self.moves.push(m);
        self.last_move = Some((from, to));
        MoveEvent {
            from,
            to,
            piece,
            capture: m.is_capture(),
            check: self.position.is_check(),
        }
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
        assert_eq!(g.san_moves(), ["O-O"]);
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
        assert_eq!(g.san_moves(), ["f3", "e5", "g4", "Qh4#"]);
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
    fn rebuilds_from_lichess_moves() {
        // Lichess streams castling king-to-rook; promotions carry the piece letter.
        let (g, last) = Game::from_uci("startpos", "e2e4 e7e5 g1f3 b8c6 f1c4 g8f6 e1h1").unwrap();
        assert_eq!(g.ply(), 7);
        assert_eq!(g.san_moves().last().map(String::as_str), Some("O-O"));
        assert_eq!(g.last_uci().as_deref(), Some("e1g1"));
        assert_eq!(last.unwrap().to, G1);

        let (g, _) = Game::from_uci("8/P7/8/8/8/8/8/k6K w - - 0 1", "a7a8n").unwrap();
        assert_eq!(g.board().piece_at(A8).map(|p| p.role), Some(Role::Knight));

        let (g, last) = Game::from_uci("startpos", "").unwrap();
        assert!(last.is_none() && g.ply() == 0);

        assert!(matches!(
            Game::from_uci("startpos", "e2e5"),
            Err(Error::IllegalMove)
        ));
        assert!(matches!(
            Game::from_uci("startpos", "zz"),
            Err(Error::Uci(_))
        ));
    }

    #[test]
    fn steps_back_through_history() {
        let (g, _) = Game::from_uci("startpos", "e2e4 e7e5 g1f3").unwrap();
        let start = g.at(0);
        assert_eq!(start.ply(), 0);
        assert_eq!(start.board(), Game::default().board());
        assert!(start.last_move().is_none());
        let two = g.at(2);
        assert_eq!(two.san_moves(), ["e4", "e5"]);
        assert_eq!(two.last_move(), Some((E7, E5)));
        assert_eq!(g.at(99).ply(), 3); // clamped
        // From a FEN start, history replays from that FEN.
        let fen = "8/P7/8/8/8/8/8/k6K w - - 0 1";
        let (g, _) = Game::from_uci(fen, "a7a8q").unwrap();
        assert_eq!(
            g.at(0).board().piece_at(A7).map(|p| p.role),
            Some(Role::Pawn)
        );
    }

    #[test]
    fn premove_targets_follow_movement_patterns() {
        let mut g = Game::default();
        g.play(E2, E4, None).unwrap(); // black to move; white premoves
        // Knight: its pattern, minus own pieces.
        assert_eq!(g.premove_targets(G1), vec![E2, F3, H3]); // e2 is free after e4
        // Bishop through its own pawn's old square: blockers are ignored...
        assert!(g.premove_targets(F1).contains(&A6));
        // ...but it can't land on its own pieces.
        assert!(!g.premove_targets(F1).contains(&G2));
        // Pawn: pushes and both diagonals (a piece may arrive there).
        assert_eq!(g.premove_targets(D2), vec![C3, D3, E3, D4]);
        // A pawn that has moved can't double-push.
        assert_eq!(g.premove_targets(E4), vec![D5, E5, F5]);
        // Castling squares while rights remain (black to move, white premoves).
        let castle = Game::from_fen("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1").unwrap();
        assert!(castle.premove_targets(E1).contains(&G1));
        assert!(castle.premove_targets(E1).contains(&C1));
        assert!(castle.premove_targets(B5).is_empty()); // empty square
    }

    #[test]
    fn bad_fen() {
        assert!(matches!(Game::from_fen("nonsense"), Err(Error::Fen(_))));
    }
}
