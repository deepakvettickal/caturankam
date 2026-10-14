mod board;

use board::{BoardView, piece_image, rgb};
use caturankam_core::{Color, Game, KnownOutcome, Outcome, Piece, Role, Square};
use caturankam_theme::Theme;
use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, RichText, Stroke, Vec2};

/// Below this window width the move list panel is hidden.
const SIDE_PANEL_MIN_WIDTH: f32 = 760.0;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Caturankam")
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([360.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Caturankam",
        options,
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}

struct App {
    game: Game,
    board: BoardView,
    theme: Theme,
    /// A pawn move waiting for the promotion piece to be chosen.
    promoting: Option<(Square, Square)>,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        egui_extras::install_image_loaders(ctx);
        let theme = Theme::original();
        apply_style(ctx, &theme);
        Self {
            game: Game::default(),
            board: BoardView::default(),
            theme,
            promoting: None,
        }
    }

    fn play(&mut self, from: Square, to: Square, promotion: Option<Role>, animate: bool, now: f64) {
        // The board only offers legal targets, so this can't fail in practice;
        // if it ever does, ignoring the input is the right behaviour.
        if let Ok(ev) = self.game.play(from, to, promotion)
            && animate
        {
            self.board.animate(&ev, now);
        }
    }

    fn new_game(&mut self) {
        self.game = Game::default();
        self.board.reset();
        self.promoting = None;
    }

    fn status(&self) -> (String, &'static str) {
        let side = |c: Color| if c == Color::White { "White" } else { "Black" };
        match self.game.outcome() {
            Outcome::Known(KnownOutcome::Decisive { winner }) => {
                (format!("{} wins", side(winner)), "Checkmate")
            }
            Outcome::Known(KnownOutcome::Draw) => ("Draw".into(), "Game over"),
            Outcome::Unknown if self.game.is_check() => {
                (format!("{} to move", side(self.game.turn())), "Check")
            }
            Outcome::Unknown => (format!("{} to move", side(self.game.turn())), "Local game"),
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::new()
            .fill(rgb(self.theme.ui.panel))
            .inner_margin(Margin::symmetric(16, 10));
        egui::Panel::top("top")
            .frame(frame)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Caturankam").size(20.0).strong());
                    ui.add_space(6.0);
                    let accent = rgb(self.theme.ui.accent);
                    egui::Frame::new()
                        .stroke(Stroke::new(1.0, accent))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(8, 2))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&self.theme.name).size(12.0).color(accent));
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("New game").clicked() {
                            self.new_game();
                        }
                        if ui.button("Flip board").clicked() {
                            self.board.flipped = !self.board.flipped;
                        }
                    });
                });
            });
    }

    fn side_panel(&self, ui: &mut egui::Ui) {
        let frame = egui::Frame::new()
            .fill(rgb(self.theme.ui.panel))
            .inner_margin(Margin::same(16));
        egui::Panel::right("side")
            .frame(frame)
            .exact_size(240.0)
            .resizable(false)
            .show_separator_line(false)
            .show(ui, |ui| {
                let (headline, label) = self.status();
                ui.label(RichText::new(label.to_uppercase()).size(11.0).weak());
                ui.label(RichText::new(headline).size(22.0).strong());
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(4.0);
                self.move_list(ui);
            });
    }

    fn move_list(&self, ui: &mut egui::Ui) {
        let moves = self.game.san_moves();
        if moves.is_empty() {
            ui.label(RichText::new("No moves yet").weak());
            return;
        }
        let mono = FontId::monospace(14.0);
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Grid::new("moves")
                    .num_columns(3)
                    .spacing([16.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for (n, pair) in moves.chunks(2).enumerate() {
                            ui.label(
                                RichText::new(format!("{}.", n + 1))
                                    .font(mono.clone())
                                    .weak(),
                            );
                            for san in pair {
                                ui.label(RichText::new(san).font(mono.clone()));
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    fn promotion_picker(&mut self, ctx: &egui::Context, now: f64) {
        let Some((from, to)) = self.promoting else {
            return;
        };
        let color = self.game.turn();
        let modal = egui::Modal::new("promotion".into()).show(ctx, |ui| {
            ui.label("Promote to");
            ui.horizontal(|ui| {
                [Role::Queen, Role::Rook, Role::Bishop, Role::Knight]
                    .into_iter()
                    .find(|&role| {
                        let img = piece_image(&self.theme, Piece { color, role })
                            .fit_to_exact_size(Vec2::splat(56.0));
                        ui.add(egui::Button::image(img))
                            .on_hover_text(format!("{role:?}"))
                            .clicked()
                    })
            })
            .inner
        });
        if let Some(role) = modal.inner {
            self.promoting = None;
            self.play(from, to, Some(role), true, now);
        } else if modal.should_close() {
            self.promoting = None; // cancelled: the pawn stays put
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = ui.input(|i| i.time);
        let wide = ui.available_width() >= SIDE_PANEL_MIN_WIDTH;

        self.top_bar(ui);
        if wide {
            self.side_panel(ui);
        }

        let frame = egui::Frame::new()
            .fill(rgb(self.theme.ui.background))
            .inner_margin(Margin::same(24));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            if !wide {
                let (headline, _) = self.status();
                ui.vertical_centered(|ui| ui.label(RichText::new(headline).size(16.0)));
                ui.add_space(8.0);
            }
            let playable = self.promoting.is_none() && self.game.outcome() == Outcome::Unknown;
            if let Some(i) = self.board.show(ui, &self.game, &self.theme, playable) {
                if self.game.is_promotion(i.from, i.to) {
                    self.promoting = Some((i.from, i.to));
                } else {
                    self.play(i.from, i.to, None, !i.dragged, now);
                }
            }
        });

        self.promotion_picker(ui.ctx(), now);
    }
}

/// Window chrome from the theme's `[ui]` palette, on top of egui's dark style.
fn apply_style(ctx: &egui::Context, theme: &Theme) {
    let pal = &theme.ui;
    let mut v = egui::Visuals::dark();
    v.panel_fill = rgb(pal.background);
    v.window_fill = rgb(pal.panel);
    v.extreme_bg_color = rgb(pal.background);
    v.faint_bg_color = rgb(pal.background).gamma_multiply(1.4);
    v.override_text_color = Some(rgb(pal.text));
    v.selection.bg_fill = rgb(pal.accent);
    v.window_stroke = Stroke::NONE;
    let button = Color32::from_white_alpha(14);
    for (w, fill) in [
        (&mut v.widgets.inactive, button),
        (&mut v.widgets.hovered, Color32::from_white_alpha(28)),
        (&mut v.widgets.active, rgb(pal.accent)),
    ] {
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.corner_radius = CornerRadius::same(6);
        w.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
    });
}
