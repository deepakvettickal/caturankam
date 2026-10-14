mod board;

use board::BoardView;
use caturankam_core::{Color, Game, KnownOutcome, Outcome, Role, Square};
use caturankam_theme::Theme;
use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Caturankam")
            .with_inner_size([640.0, 680.0])
            .with_min_inner_size([320.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Caturankam",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
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
    fn new() -> Self {
        Self {
            game: Game::default(),
            board: BoardView::default(),
            theme: Theme::original(),
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

    fn status(&self) -> String {
        let side = |c: Color| if c == Color::White { "White" } else { "Black" };
        match self.game.outcome() {
            Outcome::Known(KnownOutcome::Decisive { winner }) => {
                format!("Checkmate · {} wins", side(winner))
            }
            Outcome::Known(KnownOutcome::Draw) => "Draw".into(),
            Outcome::Unknown if self.game.is_check() => {
                format!("Check · {} to move", side(self.game.turn()))
            }
            Outcome::Unknown => format!("{} to move", side(self.game.turn())),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = ui.input(|i| i.time);

        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Caturankam");
                ui.label(&self.theme.name);
                ui.separator();
                if ui.button("New game").clicked() {
                    self.game = Game::default();
                    self.board.reset();
                    self.promoting = None;
                }
                if ui.button("Flip").clicked() {
                    self.board.flipped = !self.board.flipped;
                }
                ui.separator();
                ui.label(self.status());
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let playable = self.promoting.is_none() && self.game.outcome() == Outcome::Unknown;
            if let Some(i) = self.board.show(ui, &self.game, &self.theme, playable) {
                if self.game.is_promotion(i.from, i.to) {
                    self.promoting = Some((i.from, i.to));
                } else {
                    self.play(i.from, i.to, None, !i.dragged, now);
                }
            }
        });

        if let Some((from, to)) = self.promoting {
            let modal = egui::Modal::new("promotion".into()).show(ui.ctx(), |ui| {
                ui.label("Promote to");
                ui.horizontal(|ui| {
                    [Role::Queen, Role::Rook, Role::Bishop, Role::Knight]
                        .into_iter()
                        .find(|&role| ui.button(format!("{role:?}")).clicked())
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
}
