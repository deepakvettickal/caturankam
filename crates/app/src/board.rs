//! The board widget: draws a `Game` with the active theme and turns pointer
//! input into move intents. It never mutates the game itself.

use crate::assets::{BoardImage, PieceSet};
use caturankam_core::{Color, File, Game, MoveEvent, Piece, Rank, Role, Square};
use caturankam_theme::{Coordinates, Rgb, Theme};
use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shadow, Stroke, Vec2,
};

/// Everything visual the board needs: the mode's theme plus the user's
/// chosen piece set and (optional) board image.
pub struct Look<'a> {
    pub theme: &'a Theme,
    pub pieces: &'a PieceSet,
    pub board: Option<&'a BoardImage>,
}

/// A move the user asked for. The app decides whether to play it.
pub struct Intent {
    pub from: Square,
    pub to: Square,
    /// Dropped by drag: the piece is already under the pointer, so no animation.
    pub dragged: bool,
}

struct Anim {
    piece: Piece,
    from: Square,
    to: Square,
    start: f64,
}

#[derive(Default)]
pub struct BoardView {
    pub flipped: bool,
    /// A move queued during the opponent's turn, played the instant it's
    /// ours (if still legal then).
    pub premove: Option<(Square, Square)>,
    selected: Option<Square>,
    /// The side that may pick up pieces this frame (for drawing targets).
    side: Option<Color>,
    dragging: Option<Square>,
    anim: Option<Anim>,
}

impl BoardView {
    pub fn reset(&mut self) {
        *self = Self {
            flipped: self.flipped,
            ..Self::default()
        };
    }

    /// Animation is purely cosmetic: it starts after the move is already played
    /// and never blocks input (a new move simply replaces it).
    pub fn animate(&mut self, ev: &MoveEvent, now: f64) {
        self.anim = Some(Anim {
            piece: ev.piece,
            from: ev.from,
            to: ev.to,
            start: now,
        });
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        game: &Game,
        look: &Look,
        movable: Option<Color>,
    ) -> Option<Intent> {
        let rect = board_rect(ui.available_rect_before_wrap());
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let geo = Geometry {
            rect,
            flipped: self.flipped,
        };

        // `movable` may be the side not to move: then a finished move becomes
        // a premove instead of an intent. Right-click cancels a premove.
        if resp.secondary_clicked() {
            self.premove = None;
            self.selected = None;
        }
        self.side = movable;
        let intent = if let Some(side) = movable {
            self.handle_input(&resp, &geo, game, side)
        } else {
            self.selected = None;
            self.dragging = None;
            None
        };

        let now = ui.input(|i| i.time);
        let anim_t = self.anim.as_ref().map(|a| {
            ((now - a.start) * 1000.0 / f64::from(look.theme.animation.move_ms.max(1))).min(1.0)
                as f32
        });
        if anim_t == Some(1.0) {
            self.anim = None;
        } else if anim_t.is_some() {
            ui.ctx().request_repaint(); // only while animating: idle CPU stays near zero
        }

        let pointer = resp
            .interact_pointer_pos()
            .or(ui.input(|i| i.pointer.latest_pos()));
        self.paint(ui, &geo, game, look, anim_t, pointer);
        intent
    }

    fn handle_input(
        &mut self,
        resp: &egui::Response,
        geo: &Geometry,
        game: &Game,
        side: Color,
    ) -> Option<Intent> {
        let pos = resp.interact_pointer_pos()?;
        let sq = geo.square_at(pos);
        let own = |s: Square| game.board().piece_at(s).is_some_and(|p| p.color == side);
        let premoving = side != game.turn();
        let targets = |from: Square| targets(game, side, from);
        // A finished move: played now, or queued as the premove.
        let mut finish = |from: Square, to: Square, dragged: bool| {
            if premoving {
                self.premove = Some((from, to));
                None
            } else {
                Some(Intent { from, to, dragged })
            }
        };

        if resp.drag_started() {
            self.dragging = sq.filter(|&s| own(s));
            if self.dragging.is_some() {
                self.selected = self.dragging;
            }
        }
        if resp.drag_stopped() {
            let from = self.dragging.take()?;
            let to = sq?;
            if to == from {
                return None; // dropped back: keep it selected for click-to-move
            }
            self.selected = None;
            if !targets(from).contains(&to) {
                return None;
            }
            return finish(from, to, true);
        }
        if resp.clicked() {
            let sq = sq?;
            if let Some(from) = self.selected.filter(|&from| targets(from).contains(&sq)) {
                self.selected = None;
                return finish(from, sq, false);
            }
            self.selected = (own(sq) && self.selected != Some(sq)).then_some(sq);
        }
        None
    }

    fn paint(
        &self,
        ui: &egui::Ui,
        geo: &Geometry,
        game: &Game,
        look: &Look,
        anim_t: Option<f32>,
        pointer: Option<Pos2>,
    ) {
        let p = ui.painter();
        let b = &look.theme.board;
        let size = geo.square_size();

        if let Some(img) = look.board {
            // No spinner while a new board decodes: the plain squares show until it's ready.
            egui::Image::from_bytes(img.uri.clone(), img.bytes.clone())
                .show_loading_spinner(false)
                .paint_at(ui, geo.rect);
        }

        for sq in Square::ALL {
            let r = geo.square_rect(sq);
            let is_dark = (u32::from(sq.file()) + u32::from(sq.rank())).is_multiple_of(2);
            if look.board.is_none() {
                p.rect_filled(r, 0.0, rgb(if is_dark { b.dark } else { b.light }));
            }

            if game.last_move().is_some_and(|(f, t)| sq == f || sq == t) {
                p.rect_filled(r, 0.0, rgba(b.highlight, 120));
            }
            if self.selected == Some(sq) {
                p.rect_filled(r, 0.0, rgba(b.highlight, 190));
            }
            if self.premove.is_some_and(|(f, t)| sq == f || sq == t) {
                p.rect_filled(r, 0.0, PREMOVE);
            }
            if b.coordinates == Coordinates::Subtle {
                // Board images don't tell us their colours: use contrast ink instead.
                let ink = match (look.board, is_dark) {
                    (None, true) => rgb(b.light),
                    (None, false) => rgb(b.dark),
                    (Some(_), true) => Color32::from_white_alpha(210),
                    (Some(_), false) => Color32::from_black_alpha(150),
                };
                paint_coordinates(p, geo, sq, ink);
            }
        }

        if game.is_check()
            && let Some(king) = game.board().king_of(game.turn())
        {
            // A soft red glow: stacked translucent discs fake a radial gradient.
            let c = geo.square_rect(king).center();
            for i in 1..=5 {
                let radius = size * (0.18 + 0.065 * i as f32);
                p.circle_filled(c, radius, Color32::from_rgba_unmultiplied(230, 30, 30, 34));
            }
        }

        // Legal targets for the selected piece; the square under a drag is shaded.
        if let (Some(from), Some(side)) = (self.selected, self.side) {
            let dot = Color32::from_black_alpha(55);
            let hover = self
                .dragging
                .and(pointer)
                .and_then(|pos| geo.square_at(pos));
            for to in targets(game, side, from) {
                let r = geo.square_rect(to);
                if hover == Some(to) {
                    p.rect_filled(r, 0.0, Color32::from_black_alpha(35));
                } else if game.board().piece_at(to).is_some() {
                    p.circle_stroke(r.center(), size * 0.46, Stroke::new(size * 0.06, dot));
                } else {
                    p.circle_filled(r.center(), size * 0.14, dot);
                }
            }
        }

        let anim = self.anim.as_ref().zip(anim_t);
        for (sq, piece) in game.board().clone() {
            let hidden = self.dragging == Some(sq) || anim.is_some_and(|(a, _)| a.to == sq);
            if !hidden {
                draw_piece(ui, look.pieces, geo.square_rect(sq).center(), size, piece);
            }
        }

        if let Some((a, t)) = anim {
            let eased = -((std::f32::consts::PI * t).cos() - 1.0) / 2.0; // ease-in-out sine
            let from = geo.square_rect(a.from).center();
            let to = geo.square_rect(a.to).center();
            draw_piece(ui, look.pieces, from + (to - from) * eased, size, a.piece);
        }
        if let (Some(sq), Some(pos)) = (self.dragging, pointer)
            && let Some(piece) = game.board().piece_at(sq)
        {
            // A small shadow under the lifted piece.
            let lift = Shadow {
                offset: [0, 4],
                blur: 14,
                spread: 0,
                color: Color32::from_black_alpha(110),
            };
            let footprint = Rect::from_center_size(
                pos + Vec2::new(0.0, size * 0.32),
                Vec2::new(size * 0.55, size * 0.12),
            );
            p.add(lift.as_shape(footprint, u8::MAX));
            draw_piece(ui, look.pieces, pos, size, piece);
        }
    }
}

fn paint_coordinates(p: &Painter, geo: &Geometry, sq: Square, ink: Color32) {
    let r = geo.square_rect(sq);
    let size = geo.square_size();
    let font = FontId::proportional((size * 0.17).max(9.0));
    let pad = size * 0.07;
    if geo.view_row(sq) == 7 {
        let pos = r.right_bottom() - Vec2::new(pad, pad * 0.6);
        p.text(
            pos,
            Align2::RIGHT_BOTTOM,
            sq.file().char(),
            font.clone(),
            ink,
        );
    }
    if geo.view_col(sq) == 0 {
        let pos = r.left_top() + Vec2::new(pad * 0.8, pad * 0.5);
        p.text(pos, Align2::LEFT_TOP, sq.rank().char(), font, ink);
    }
}

/// The largest square board centred in `area`. Whole points per square keep
/// square edges and piece rasters crisp.
pub fn board_rect(area: Rect) -> Rect {
    let side = (area.width().min(area.height()) / 8.0).floor() * 8.0;
    let min = (area.center() - Vec2::splat(side / 2.0)).round();
    Rect::from_min_size(min, Vec2::splat(side))
}

/// Premove squares: a cool tint, distinct from the last-move highlight.
const PREMOVE: Color32 = Color32::from_rgba_premultiplied(40, 70, 120, 110);

/// Where `side`'s piece on `from` can go: legal moves on its turn, the
/// movement pattern (for a premove) otherwise.
fn targets(game: &Game, side: Color, from: Square) -> Vec<Square> {
    if side == game.turn() {
        game.targets(from)
    } else {
        game.premove_targets(from)
    }
}

/// Maps squares to screen space. White is at the bottom unless flipped.
struct Geometry {
    rect: Rect,
    flipped: bool,
}

impl Geometry {
    fn square_size(&self) -> f32 {
        self.rect.width() / 8.0
    }

    fn view_col(&self, sq: Square) -> u32 {
        let f = u32::from(sq.file());
        if self.flipped { 7 - f } else { f }
    }

    fn view_row(&self, sq: Square) -> u32 {
        let r = u32::from(sq.rank());
        if self.flipped { r } else { 7 - r }
    }

    fn square_rect(&self, sq: Square) -> Rect {
        let s = self.square_size();
        let min = self.rect.min + Vec2::new(self.view_col(sq) as f32, self.view_row(sq) as f32) * s;
        Rect::from_min_size(min, Vec2::splat(s))
    }

    fn square_at(&self, pos: Pos2) -> Option<Square> {
        if !self.rect.contains(pos) {
            return None;
        }
        let v = (pos - self.rect.min) / self.square_size();
        let (col, row) = ((v.x as u32).min(7), (v.y as u32).min(7));
        let (f, r) = if self.flipped {
            (7 - col, row)
        } else {
            (col, 7 - row)
        };
        Some(Square::from_coords(File::new(f), Rank::new(r)))
    }
}

/// Paints a piece from the SVG set, centred on `center`.
fn draw_piece(ui: &egui::Ui, set: &PieceSet, center: Pos2, size: f32, piece: Piece) {
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    piece_image(set, piece).paint_at(ui, rect);
}

pub fn piece_image(set: &PieceSet, piece: Piece) -> egui::Image<'static> {
    let role = match piece.role {
        Role::King => 0,
        Role::Queen => 1,
        Role::Rook => 2,
        Role::Bishop => 3,
        Role::Knight => 4,
        Role::Pawn => 5,
    };
    let i = role + if piece.color == Color::White { 0 } else { 6 };
    // The URI keys egui's raster cache, so each set has its own. No spinner
    // while a new set rasterises: it appears as soon as it's ready.
    egui::Image::from_bytes(set.uris[i].clone(), set.svgs[i].clone()).show_loading_spinner(false)
}

fn rgb(c: Rgb) -> Color32 {
    let [r, g, b] = c.0;
    Color32::from_rgb(r, g, b)
}

fn rgba(c: Rgb, a: u8) -> Color32 {
    let [r, g, b] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_round_trips() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::splat(800.0));
        for flipped in [false, true] {
            let geo = Geometry { rect, flipped };
            for sq in Square::ALL {
                assert_eq!(geo.square_at(geo.square_rect(sq).center()), Some(sq));
            }
        }
        let geo = Geometry {
            rect,
            flipped: false,
        };
        assert_eq!(
            geo.square_at(rect.left_bottom() + Vec2::new(1.0, -1.0)),
            Some(Square::A1)
        );
        assert_eq!(geo.square_at(Pos2::ZERO), None);
    }
}
