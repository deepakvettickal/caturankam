//! The board widget: draws a `Game` with the active theme and turns pointer
//! input into move intents. It never mutates the game itself.

use caturankam_core::{Color, File, Game, MoveEvent, Piece, Rank, Role, Square};
use caturankam_theme::{Coordinates, PIECES, Rgb, Theme};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Shadow, Stroke, Vec2,
};

const CORNER: u8 = 6;

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
        // Whole points per square keeps square edges and piece rasters crisp.
        let side = (avail.width().min(avail.height()) / 8.0).floor() * 8.0;
        let min = (avail.center() - Vec2::splat(side / 2.0)).round();
        let rect = Rect::from_min_size(min, Vec2::splat(side));
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
        self.paint(ui, &geo, game, theme, anim_t, pointer);
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
        ui: &egui::Ui,
        geo: &Geometry,
        game: &Game,
        theme: &Theme,
        anim_t: Option<f32>,
        pointer: Option<Pos2>,
    ) {
        let p = ui.painter();
        let b = &theme.board;
        let size = geo.square_size();

        let shadow = Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(140),
        };
        p.add(shadow.as_shape(geo.rect, CORNER));

        for sq in Square::ALL {
            let r = geo.square_rect(sq);
            let round = geo.corner_radius(sq);
            let is_dark = (u32::from(sq.file()) + u32::from(sq.rank())).is_multiple_of(2);
            p.rect_filled(r, round, rgb(if is_dark { b.dark } else { b.light }));

            if game.last_move().is_some_and(|(f, t)| sq == f || sq == t) {
                p.rect_filled(r, round, rgba(b.highlight, 120));
            }
            if self.selected == Some(sq) {
                p.rect_filled(r, round, rgba(b.highlight, 190));
            }
            if b.coordinates == Coordinates::Subtle {
                paint_coordinates(p, geo, sq, rgb(if is_dark { b.light } else { b.dark }));
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
        if let Some(from) = self.selected {
            let dot = Color32::from_black_alpha(55);
            let hover = self
                .dragging
                .and(pointer)
                .and_then(|pos| geo.square_at(pos));
            for to in game.targets(from) {
                let r = geo.square_rect(to);
                if hover == Some(to) {
                    p.rect_filled(r, geo.corner_radius(to), Color32::from_black_alpha(35));
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
                draw_piece(ui, theme, geo.square_rect(sq).center(), size, piece);
            }
        }

        if let Some((a, t)) = anim {
            let eased = -((std::f32::consts::PI * t).cos() - 1.0) / 2.0; // ease-in-out sine
            let from = geo.square_rect(a.from).center();
            let to = geo.square_rect(a.to).center();
            draw_piece(ui, theme, from + (to - from) * eased, size, a.piece);
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
            draw_piece(ui, theme, pos, size, piece);
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

    /// Rounds only the board's four outer corners.
    fn corner_radius(&self, sq: Square) -> CornerRadius {
        let at = |c, r| {
            if (self.view_col(sq), self.view_row(sq)) == (c, r) {
                CORNER
            } else {
                0
            }
        };
        CornerRadius {
            nw: at(0, 0),
            ne: at(7, 0),
            sw: at(0, 7),
            se: at(7, 7),
        }
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

/// Paints a piece from the theme's SVG set, centred on `center`.
fn draw_piece(ui: &egui::Ui, theme: &Theme, center: Pos2, size: f32, piece: Piece) {
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    piece_image(theme, piece).paint_at(ui, rect);
}

pub fn piece_image(theme: &Theme, piece: Piece) -> egui::Image<'static> {
    let role = match piece.role {
        Role::King => 0,
        Role::Queen => 1,
        Role::Rook => 2,
        Role::Bishop => 3,
        Role::Knight => 4,
        Role::Pawn => 5,
    };
    let i = role + if piece.color == Color::White { 0 } else { 6 };
    // The URI keys egui's raster cache, so it must be unique per theme and piece.
    let uri = format!("bytes://{}/{}.svg", theme.id, PIECES[i]);
    egui::Image::from_bytes(uri, theme.pieces[i])
}

pub fn rgb(c: Rgb) -> Color32 {
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
        assert_eq!(geo.corner_radius(Square::A1).sw, CORNER);
        assert_eq!(geo.corner_radius(Square::E4), CornerRadius::ZERO);
    }
}
