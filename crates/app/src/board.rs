//! The board widget: draws a `Game` with the active theme and turns pointer
//! input into move intents. It never mutates the game itself.

use caturankam_core::{Color, File, Game, MoveEvent, Piece, Rank, Role, Square};
use caturankam_theme::{Coordinates, Rgb, Theme};
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Vec2};

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
    selected: Option<Square>,
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
        theme: &Theme,
        interactive: bool,
    ) -> Option<Intent> {
        let avail = ui.available_rect_before_wrap();
        let rect = Rect::from_center_size(
            avail.center(),
            Vec2::splat(avail.width().min(avail.height())),
        );
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let geo = Geometry {
            rect,
            flipped: self.flipped,
        };

        let intent = if interactive {
            self.handle_input(&resp, &geo, game)
        } else {
            self.selected = None;
            self.dragging = None;
            None
        };

        let now = ui.input(|i| i.time);
        let anim_t = self.anim.as_ref().map(|a| {
            ((now - a.start) * 1000.0 / f64::from(theme.animation.move_ms.max(1))).min(1.0) as f32
        });
        if anim_t == Some(1.0) {
            self.anim = None;
        } else if anim_t.is_some() {
            ui.ctx().request_repaint(); // only while animating: idle CPU stays near zero
        }

        let pointer = resp
            .interact_pointer_pos()
            .or(ui.input(|i| i.pointer.latest_pos()));
        self.paint(ui.painter(), &geo, game, theme, anim_t, pointer);
        intent
    }

    fn handle_input(
        &mut self,
        resp: &egui::Response,
        geo: &Geometry,
        game: &Game,
    ) -> Option<Intent> {
        let pos = resp.interact_pointer_pos()?;
        let sq = geo.square_at(pos);
        let own = |s: Square| {
            game.board()
                .piece_at(s)
                .is_some_and(|p| p.color == game.turn())
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
            return game.targets(from).contains(&to).then_some(Intent {
                from,
                to,
                dragged: true,
            });
        }
        if resp.clicked() {
            let sq = sq?;
            if let Some(from) = self
                .selected
                .filter(|&from| game.targets(from).contains(&sq))
            {
                self.selected = None;
                return Some(Intent {
                    from,
                    to: sq,
                    dragged: false,
                });
            }
            self.selected = (own(sq) && self.selected != Some(sq)).then_some(sq);
        }
        None
    }

    fn paint(
        &self,
        p: &Painter,
        geo: &Geometry,
        game: &Game,
        theme: &Theme,
        anim_t: Option<f32>,
        pointer: Option<Pos2>,
    ) {
        let b = &theme.board;
        let highlight = rgba(b.highlight, 140);
        let size = geo.square_size();

        for sq in Square::ALL {
            let r = geo.square_rect(sq);
            let is_dark = (u32::from(sq.file()) + u32::from(sq.rank())) % 2 == 0;
            p.rect_filled(r, 0.0, rgb(if is_dark { b.dark } else { b.light }));

            if self.selected == Some(sq)
                || game.last_move().is_some_and(|(f, t)| sq == f || sq == t)
            {
                p.rect_filled(r, 0.0, highlight);
            }
            if b.coordinates == Coordinates::Subtle {
                let ink = rgb(if is_dark { b.light } else { b.dark });
                let font = FontId::proportional(size * 0.16);
                let pad = size * 0.06;
                if geo.view_row(sq) == 7 {
                    p.text(
                        r.right_bottom() - Vec2::splat(pad),
                        Align2::RIGHT_BOTTOM,
                        sq.file().char(),
                        font.clone(),
                        ink,
                    );
                }
                if geo.view_col(sq) == 0 {
                    p.text(
                        r.left_top() + Vec2::splat(pad),
                        Align2::LEFT_TOP,
                        sq.rank().char(),
                        font,
                        ink,
                    );
                }
            }
        }

        if game.is_check()
            && let Some(king) = game.board().king_of(game.turn())
        {
            p.circle_filled(
                geo.square_rect(king).center(),
                size * 0.45,
                Color32::from_rgba_unmultiplied(220, 40, 40, 110),
            );
        }

        let anim = self.anim.as_ref().zip(anim_t);
        for (sq, piece) in game.board().clone() {
            let hidden = self.dragging == Some(sq) || anim.is_some_and(|(a, _)| a.to == sq);
            if !hidden {
                draw_piece(p, geo.square_rect(sq).center(), size, piece);
            }
        }

        if let Some(from) = self.selected {
            for to in game.targets(from) {
                let c = geo.square_rect(to).center();
                let dot = Color32::from_black_alpha(60);
                if game.board().piece_at(to).is_some() {
                    p.circle_stroke(c, size * 0.46, Stroke::new(size * 0.07, dot));
                } else {
                    p.circle_filled(c, size * 0.15, dot);
                }
            }
        }

        if let Some((a, t)) = anim {
            let eased = -((std::f32::consts::PI * t).cos() - 1.0) / 2.0; // ease-in-out sine
            let from = geo.square_rect(a.from).center();
            let to = geo.square_rect(a.to).center();
            draw_piece(p, from + (to - from) * eased, size, a.piece);
        }
        if let (Some(sq), Some(pos)) = (self.dragging, pointer)
            && let Some(piece) = game.board().piece_at(sq)
        {
            draw_piece(p, pos, size * 1.1, piece);
        }
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

/// Placeholder pieces drawn in code until real art exists: a disc with the
/// piece letter. Crude, but legible at any size.
fn draw_piece(p: &Painter, center: Pos2, size: f32, piece: Piece) {
    let (fill, ink) = match piece.color {
        Color::White => (
            Color32::from_rgb(250, 250, 250),
            Color32::from_rgb(30, 30, 30),
        ),
        Color::Black => (
            Color32::from_rgb(35, 35, 35),
            Color32::from_rgb(240, 240, 240),
        ),
    };
    let radius = if piece.role == Role::Pawn {
        size * 0.3
    } else {
        size * 0.38
    };
    p.circle(center, radius, fill, Stroke::new(size * 0.03, ink));
    let letter = piece.role.upper_char().to_string();
    p.text(
        center,
        Align2::CENTER_CENTER,
        letter,
        FontId::proportional(radius * 1.15),
        ink,
    );
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
