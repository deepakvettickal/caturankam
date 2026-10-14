mod assets;
mod board;
mod net;
mod online;
mod settings;

use assets::{Assets, BOARDS, Fetch, PIECE_SETS, PieceSet};
use board::{BoardView, Look, piece_image, rgb};
use caturankam_core::{Color, Game, KnownOutcome, Outcome, Piece, Role, Square};
use caturankam_lichess::{GameEvent, Side};
use caturankam_theme::Theme;
use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, RichText, Stroke, Vec2};
use net::{Cmd, Evt, Net};
use online::{OnlineGame, format_clock, next_tick, player_label};
use settings::Settings;
use std::time::Instant;

/// Rapid and Classical: the only real-time controls Lichess allows Board API
/// clients to seek. (minutes, increment)
const SEEKS: &[(u32, u32)] = &[(10, 0), (10, 5), (15, 10), (30, 0), (30, 20)];
/// Against the computer, Blitz is allowed too.
const AI_CLOCKS: &[(u32, u32)] = &[(3, 2), (5, 0), (5, 3), (10, 0), (10, 5), (15, 10)];
const NOTICE_SECS: f64 = 6.0;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Caturankam")
            .with_inner_size([1000.0, 720.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Caturankam",
        options,
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}

enum Account {
    LoggedOut,
    LoggingIn,
    Connecting(String),
    LoggedIn(String),
}

enum Play {
    Local,
    Seeking {
        label: String,
        ended_at: Option<f64>,
    },
    Online(Box<OnlineGame>),
}

struct Lobby {
    rated: bool,
    ai_level: u8,
    ai_color: Option<Side>,
}

struct App {
    theme: Theme,
    builtin: PieceSet,
    assets: Assets,
    settings: Settings,
    /// Started on first need; `None` for a logged-out user with cached assets.
    net: Option<Net>,
    account: Account,
    play: Play,
    local: Game,
    board: BoardView,
    /// A pawn move waiting for the promotion piece to be chosen.
    promoting: Option<(Square, Square)>,
    lobby: Lobby,
    notice: Option<(String, f64)>,
    show_appearance: bool,
    confirm_resign: bool,
    draw_offered: bool,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        egui_extras::install_image_loaders(ctx);
        let theme = Theme::original();
        apply_style(ctx, &theme);
        let settings = Settings::load();
        let mut app = Self {
            builtin: PieceSet::builtin(&theme),
            theme,
            assets: Assets::new(),
            account: Account::LoggedOut,
            settings,
            net: None,
            play: Play::Local,
            local: Game::default(),
            board: BoardView::default(),
            promoting: None,
            lobby: Lobby {
                rated: false,
                ai_level: 3,
                ai_color: None,
            },
            notice: None,
            show_appearance: false,
            confirm_resign: false,
            draw_offered: false,
        };
        app.apply_appearance(ctx);
        if let Some(user) = app.settings.username.clone() {
            app.account = Account::Connecting(user);
            app.net(ctx).send(Cmd::Connect);
        }
        app
    }

    fn net(&mut self, ctx: &egui::Context) -> &Net {
        self.net.get_or_insert_with(|| Net::start(ctx.clone()))
    }

    /// Loads the chosen piece set and board: from disk if cached, else downloads.
    fn apply_appearance(&mut self, ctx: &egui::Context) {
        let mut fetches = self
            .assets
            .select_pieces(ctx, self.settings.piece_set.as_deref());
        fetches.extend(
            self.assets
                .select_board(ctx, self.settings.board.as_deref()),
        );
        for Fetch { url, cache } in fetches {
            self.net(ctx).send(Cmd::Fetch { url, cache });
        }
    }

    fn notify(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        let now = ctx.input(|i| i.time);
        self.notice = Some((msg.into(), now + NOTICE_SECS));
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(NOTICE_SECS));
    }

    fn game(&self) -> &Game {
        match &self.play {
            Play::Online(g) => &g.game,
            _ => &self.local,
        }
    }

    fn username(&self) -> Option<&str> {
        match &self.account {
            Account::LoggedIn(u) => Some(u),
            _ => None,
        }
    }

    // ---- network events ----

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(net) = &self.net else { return };
        let events: Vec<Evt> = net.rx.try_iter().collect();
        for evt in events {
            self.on_event(ctx, evt);
        }
        if let Play::Seeking {
            ended_at: Some(t), ..
        } = self.play
        {
            // The seek closed but no game arrived: it expired.
            if ctx.input(|i| i.time) - t > 3.0 {
                self.play = Play::Local;
                self.notify(ctx, "No opponent found. Try again.");
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }

    fn on_event(&mut self, ctx: &egui::Context, evt: Evt) {
        match evt {
            Evt::LoggedIn { username, prefs } => {
                self.settings.username = Some(username.clone());
                self.account = Account::LoggedIn(username);
                if let Some(p) = prefs {
                    // Mirror Lichess. `cburnett` and `brown` are what's built in already.
                    self.settings.piece_set = p
                        .piece_set
                        .filter(|s| s != "cburnett" && PIECE_SETS.contains(&s.as_str()));
                    self.settings.board = p
                        .theme
                        .filter(|t| t != "brown" && BOARDS.iter().any(|(n, _)| n == t));
                    self.apply_appearance(ctx);
                }
                self.settings.save();
            }
            Evt::LoggedOut { reason } => {
                self.account = Account::LoggedOut;
                self.settings.username = None;
                self.settings.save();
                if !matches!(self.play, Play::Local) {
                    self.back_to_local();
                }
                if let Some(r) = reason {
                    self.notify(ctx, r);
                }
            }
            Evt::Notice(msg) => {
                if matches!(self.account, Account::LoggingIn) {
                    self.account = Account::LoggedOut;
                }
                self.notify(ctx, msg);
            }
            Evt::SeekEnded => {
                if let Play::Seeking { ended_at, .. } = &mut self.play {
                    *ended_at = Some(ctx.input(|i| i.time));
                }
            }
            Evt::Game(ev) => self.on_game_event(ctx, ev),
            Evt::Fetched { url, bytes } => match bytes {
                Ok(b) => self.assets.fetched(&url, b),
                Err(e) => {
                    self.assets.failed(&url);
                    self.notify(ctx, format!("Download failed: {e}"));
                }
            },
        }
    }

    fn on_game_event(&mut self, ctx: &egui::Context, ev: GameEvent) {
        let now = ctx.input(|i| i.time);
        match ev {
            GameEvent::GameFull(full) => {
                let my_id = self.username().unwrap_or_default().to_lowercase();
                let same = matches!(&self.play, Play::Online(g) if g.id == full.id);
                match OnlineGame::new(*full, &my_id) {
                    Ok(g) => {
                        if !same {
                            // A new game: reset the board and face our side.
                            self.board.reset();
                            self.board.flipped = g.me == Color::Black;
                            self.promoting = None;
                            self.confirm_resign = false;
                            self.draw_offered = false;
                        }
                        self.play = Play::Online(Box::new(g));
                    }
                    Err(e) => self.notify(ctx, format!("Couldn't load the game: {e}")),
                }
            }
            GameEvent::GameState(st) => {
                let Play::Online(g) = &mut self.play else {
                    return;
                };
                // Finish with `g` (a mutable borrow of `self.play`) before touching `self` again.
                let (result, over) = (g.update(&st), g.is_over());
                if over {
                    self.promoting = None;
                }
                match result {
                    Ok(Some(ev)) => self.board.animate(&ev, now),
                    Ok(None) => {}
                    Err(e) => self.notify(ctx, format!("Out of sync with Lichess: {e}")),
                }
            }
            GameEvent::OpponentGone {
                gone,
                claim_win_in_seconds,
            } => {
                if let Play::Online(g) = &mut self.play {
                    g.opponent_gone = gone.then_some(claim_win_in_seconds.unwrap_or(0));
                }
            }
            GameEvent::Other => {}
        }
    }

    // ---- actions ----

    fn back_to_local(&mut self) {
        self.play = Play::Local;
        self.board.reset();
        self.board.flipped = false;
        self.promoting = None;
    }

    /// Plays a move the user made. Online, it's shown immediately and sent at
    /// once: animation never costs clock time.
    fn play_move(
        &mut self,
        from: Square,
        to: Square,
        promo: Option<Role>,
        animate: bool,
        now: f64,
    ) {
        let ev = match &mut self.play {
            Play::Online(g) => {
                let Ok(ev) = g.game.play(from, to, promo) else {
                    return;
                };
                if let (Some(uci), Some(net)) = (g.game.last_uci(), &self.net) {
                    net.send(Cmd::Move {
                        game: g.id.clone(),
                        uci,
                    });
                }
                ev
            }
            _ => match self.local.play(from, to, promo) {
                Ok(ev) => ev,
                Err(_) => return, // only legal targets are offered, so this can't happen
            },
        };
        if animate {
            self.board.animate(&ev, now);
        }
    }

    fn seek(&mut self, ctx: &egui::Context, minutes: u32, increment: u32) {
        let rated = self.lobby.rated;
        let kind = if minutes * 60 + increment * 40 >= 1500 {
            "Classical"
        } else {
            "Rapid"
        };
        let label = format!(
            "{minutes}+{increment} {kind} · {}",
            if rated { "Rated" } else { "Casual" }
        );
        self.play = Play::Seeking {
            label,
            ended_at: None,
        };
        self.net(ctx).send(Cmd::Seek {
            minutes,
            increment,
            rated,
        });
    }

    // ---- UI ----

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
                        self.account_widget(ui);
                        ui.separator();
                        if ui.button("Board & pieces").clicked() {
                            self.show_appearance = !self.show_appearance;
                        }
                        if ui.button("Flip board").clicked() {
                            self.board.flipped = !self.board.flipped;
                        }
                    });
                });
            });
    }

    fn account_widget(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        match &self.account {
            Account::LoggedOut => {
                if ui.button("Log in with Lichess").clicked() {
                    self.account = Account::LoggingIn;
                    self.net(&ctx).send(Cmd::Login);
                }
            }
            Account::LoggingIn => {
                if ui.button("Cancel").clicked() {
                    self.account = Account::LoggedOut;
                }
                ui.label("Finish logging in in your browser…");
                ui.spinner();
            }
            Account::Connecting(user) => {
                ui.label(RichText::new(user).weak());
                ui.spinner();
            }
            Account::LoggedIn(user) => {
                let user = user.clone();
                ui.menu_button(format!("● {user}"), |ui| {
                    if ui.button("Log out").clicked() {
                        if let Play::Seeking { .. } = self.play {
                            self.net(&ctx).send(Cmd::CancelSeek);
                        }
                        self.net(&ctx).send(Cmd::Logout);
                        ui.close();
                    }
                });
            }
        }
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::new()
            .fill(rgb(self.theme.ui.panel))
            .inner_margin(Margin::same(16));
        egui::Panel::right("side")
            .frame(frame)
            .exact_size(290.0)
            .resizable(false)
            .show_separator_line(false)
            .show(ui, |ui| {
                match &self.play {
                    Play::Local => self.local_panel(ui),
                    Play::Seeking { .. } => self.seeking_panel(ui),
                    Play::Online(_) => self.online_panel(ui),
                }
                if let Some((msg, until)) = &self.notice {
                    if ui.input(|i| i.time) < *until {
                        ui.add_space(8.0);
                        ui.label(RichText::new(msg).color(Color32::from_rgb(230, 120, 100)));
                    } else {
                        self.notice = None;
                    }
                }
            });
    }

    fn local_panel(&mut self, ui: &mut egui::Ui) {
        let (headline, label) = local_status(&self.local);
        ui.horizontal(|ui| {
            ui.label(RichText::new(label.to_uppercase()).size(11.0).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("New game").clicked() {
                    self.local = Game::default();
                    self.board.reset();
                    self.promoting = None;
                }
            });
        });
        ui.label(RichText::new(headline).size(20.0).strong());
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("local-moves")
            .max_height(140.0)
            .stick_to_bottom(true)
            .show(ui, |ui| move_list(ui, &self.local));
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        if self.username().is_none() {
            ui.label(RichText::new("PLAY ONLINE").size(11.0).weak());
            ui.label("Log in with your Lichess account to play rated and casual games.");
            return;
        }
        self.lobby(ui);
    }

    fn lobby(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let button = Vec2::new(78.0, 34.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new("QUICK PAIRING").size(11.0).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.lobby.rated, "Rated");
            });
        });
        let mut picked = None;
        egui::Grid::new("seeks").spacing([8.0, 8.0]).show(ui, |ui| {
            for (i, &(m, inc)) in SEEKS.iter().enumerate() {
                if ui
                    .add_sized(button, egui::Button::new(format!("{m}+{inc}")))
                    .clicked()
                {
                    picked = Some((m, inc));
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        if let Some((m, inc)) = picked {
            self.seek(&ctx, m, inc);
        }
        ui.label(
            RichText::new("Lichess allows only Rapid and Classical pairing from apps like this one. Blitz works against the computer.")
                .size(11.0)
                .weak(),
        );

        ui.add_space(10.0);
        ui.label(RichText::new("PLAY THE COMPUTER").size(11.0).weak());
        ui.add(egui::Slider::new(&mut self.lobby.ai_level, 1..=8).text("level"));
        ui.horizontal(|ui| {
            for (side, label) in [
                (None, "Random"),
                (Some(Side::White), "White"),
                (Some(Side::Black), "Black"),
            ] {
                ui.selectable_value(&mut self.lobby.ai_color, side, label);
            }
        });
        let mut picked = None;
        egui::Grid::new("ai").spacing([8.0, 8.0]).show(ui, |ui| {
            for (i, &(m, inc)) in AI_CLOCKS.iter().enumerate() {
                if ui
                    .add_sized(button, egui::Button::new(format!("{m}+{inc}")))
                    .clicked()
                {
                    picked = Some((m, inc));
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        if let Some((m, inc)) = picked {
            let (level, color) = (self.lobby.ai_level, self.lobby.ai_color);
            self.net(&ctx).send(Cmd::ChallengeAi {
                level,
                limit_secs: m * 60,
                increment: inc,
                color,
            });
        }
    }

    fn seeking_panel(&mut self, ui: &mut egui::Ui) {
        let Play::Seeking { label, ended_at } = &self.play else {
            return;
        };
        ui.label(RichText::new("QUICK PAIRING").size(11.0).weak());
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spinner();
            let text = if ended_at.is_some() {
                "Starting game…"
            } else {
                "Finding an opponent…"
            };
            ui.label(RichText::new(text).size(16.0));
        });
        ui.label(label.as_str());
        ui.add_space(12.0);
        if ui.button("Cancel").clicked() {
            if let Some(net) = &self.net {
                net.send(Cmd::CancelSeek);
            }
            self.play = Play::Local;
        }
    }

    fn online_panel(&mut self, ui: &mut egui::Ui) {
        let Play::Online(g) = &self.play else { return };
        let now = Instant::now();
        let accent = rgb(self.theme.ui.accent);
        let them = if g.me == Color::White {
            Color::Black
        } else {
            Color::White
        };

        let kind = if g.rated { "Rated" } else { "Casual" };
        ui.label(
            RichText::new(format!("{} · {kind}", g.speed).to_uppercase())
                .size(11.0)
                .weak(),
        );
        ui.add_space(4.0);
        player_card(ui, g, them, now, accent);
        if let Some(secs) = g.opponent_gone {
            ui.label(
                RichText::new(format!(
                    "Opponent left. You can claim victory in {secs}s on lichess.org."
                ))
                .size(11.0)
                .weak(),
            );
        }
        ui.add_space(6.0);
        let moves_height = (ui.available_height() - 150.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("online-moves")
            .max_height(moves_height)
            .stick_to_bottom(true)
            .show(ui, |ui| move_list(ui, &g.game));
        ui.add_space(6.0);
        player_card(ui, g, g.me, now, accent);
        ui.add_space(8.0);

        if g.ticking() {
            let left = g.clock(g.game.turn(), now).unwrap_or_default();
            ui.ctx().request_repaint_after(next_tick(left));
        }

        let id = g.id.clone();
        if g.is_over() {
            ui.label(RichText::new(g.result()).size(18.0).strong());
            ui.add_space(6.0);
            if ui.button("Back to lobby").clicked() {
                self.back_to_local();
            }
            return;
        }
        let (can_abort, offers_draw) = (g.can_abort(), g.opponent_offers_draw);
        let Some(net) = &self.net else { return };
        if offers_draw {
            ui.label("Your opponent offers a draw.");
            ui.horizontal(|ui| {
                if ui.button("Accept").clicked() {
                    net.send(Cmd::Draw {
                        game: id.clone(),
                        accept: true,
                    });
                }
                if ui.button("Decline").clicked() {
                    net.send(Cmd::Draw {
                        game: id.clone(),
                        accept: false,
                    });
                }
            });
        }
        ui.horizontal(|ui| {
            if can_abort {
                if ui.button("Abort").clicked() {
                    net.send(Cmd::Abort(id.clone()));
                }
                return;
            }
            let resign = if self.confirm_resign {
                "Confirm resign"
            } else {
                "Resign"
            };
            if ui.button(resign).clicked() {
                if self.confirm_resign {
                    net.send(Cmd::Resign(id.clone()));
                }
                self.confirm_resign = !self.confirm_resign;
            }
            let draw = if self.draw_offered {
                "Draw offered"
            } else {
                "Offer draw"
            };
            if ui
                .add_enabled(!self.draw_offered, egui::Button::new(draw))
                .clicked()
            {
                net.send(Cmd::Draw {
                    game: id.clone(),
                    accept: true,
                });
                self.draw_offered = true;
            }
        });
    }

    fn appearance_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_appearance;
        let mut changed = false;
        egui::Window::new("Board & pieces")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                let current = self.settings.piece_set.clone();
                egui::ComboBox::from_label("Pieces")
                    .selected_text(current.as_deref().unwrap_or("cburnett (built in)"))
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut self.settings.piece_set, None, "cburnett (built in)").changed();
                        for &set in &PIECE_SETS[1..] {
                            changed |= ui.selectable_value(&mut self.settings.piece_set, Some(set.into()), set).changed();
                        }
                    });
                ui.horizontal(|ui| {
                    if self.assets.loading() {
                        ui.spinner();
                        ui.label("Downloading…");
                    } else {
                        let set = self.piece_set();
                        for role in [Role::King, Role::Queen, Role::Rook, Role::Bishop, Role::Knight, Role::Pawn] {
                            let piece = Piece { color: Color::White, role };
                            ui.add(piece_image(set, piece).fit_to_exact_size(Vec2::splat(36.0)));
                        }
                    }
                });
                ui.add_space(6.0);
                egui::ComboBox::from_label("Board")
                    .selected_text(self.settings.board.as_deref().unwrap_or("brown (built in)"))
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut self.settings.board, None, "brown (built in)").changed();
                        for (name, _) in &BOARDS[1..] {
                            changed |= ui.selectable_value(&mut self.settings.board, Some((*name).into()), *name).changed();
                        }
                    });
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Sets and boards download from lichess.org the first time you pick them, then stay cached. Each keeps its author's licence.")
                        .size(11.0)
                        .weak(),
                );
            });
        self.show_appearance = open;
        if changed {
            self.settings.save();
            self.apply_appearance(ctx);
        }
    }

    fn piece_set(&self) -> &PieceSet {
        self.assets
            .pieces
            .as_ref()
            .map_or(&self.builtin, |(_, s)| s)
    }

    fn promotion_picker(&mut self, ctx: &egui::Context, now: f64) {
        let Some((from, to)) = self.promoting else {
            return;
        };
        let color = self.game().turn();
        let set = self.piece_set();
        let modal = egui::Modal::new("promotion".into()).show(ctx, |ui| {
            ui.label("Promote to");
            ui.horizontal(|ui| {
                [Role::Queen, Role::Rook, Role::Bishop, Role::Knight]
                    .into_iter()
                    .find(|&role| {
                        let img = piece_image(set, Piece { color, role })
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
            self.play_move(from, to, Some(role), true, now);
        } else if modal.should_close() {
            self.promoting = None; // cancelled: the pawn stays put
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ui.input(|i| i.time);
        self.poll(&ctx);

        self.top_bar(ui);
        self.side_panel(ui);

        let frame = egui::Frame::new()
            .fill(rgb(self.theme.ui.background))
            .inner_margin(Margin::same(24));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            let movable = match &self.play {
                _ if self.promoting.is_some() => None,
                Play::Local => {
                    (self.local.outcome() == Outcome::Unknown).then(|| self.local.turn())
                }
                Play::Seeking { .. } => None,
                Play::Online(g) => g.my_turn().then_some(g.me),
            };
            let look = Look {
                theme: &self.theme,
                pieces: self
                    .assets
                    .pieces
                    .as_ref()
                    .map_or(&self.builtin, |(_, s)| s),
                board: self.assets.board.as_ref().map(|(_, b)| b),
            };
            let game = match &self.play {
                Play::Online(g) => &g.game,
                _ => &self.local,
            };
            if let Some(i) = self.board.show(ui, game, &look, movable) {
                if game.is_promotion(i.from, i.to) {
                    self.promoting = Some((i.from, i.to));
                } else {
                    self.play_move(i.from, i.to, None, !i.dragged, now);
                }
            }
        });

        self.promotion_picker(&ctx, now);
        self.appearance_window(&ctx);
    }
}

fn local_status(game: &Game) -> (String, &'static str) {
    let side = |c: Color| if c == Color::White { "White" } else { "Black" };
    match game.outcome() {
        Outcome::Known(KnownOutcome::Decisive { winner }) => {
            (format!("{} wins", side(winner)), "Checkmate")
        }
        Outcome::Known(KnownOutcome::Draw) => ("Draw".into(), "Game over"),
        Outcome::Unknown if game.is_check() => (format!("{} to move", side(game.turn())), "Check"),
        Outcome::Unknown => (format!("{} to move", side(game.turn())), "Local game"),
    }
}

fn player_card(ui: &mut egui::Ui, g: &OnlineGame, c: Color, now: Instant, accent: Color32) {
    let running = g.ticking() && g.game.turn() == c;
    ui.horizontal(|ui| {
        ui.label(RichText::new(player_label(g.player(c))).size(15.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let Some(left) = g.clock(c, now) else { return };
            let low = left.as_secs() < 10;
            let fill = match (running, low) {
                (true, true) => Color32::from_rgb(170, 40, 30),
                (true, false) => accent,
                _ => Color32::from_white_alpha(14),
            };
            egui::Frame::new()
                .fill(fill)
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    let text = RichText::new(format_clock(left)).font(FontId::monospace(24.0));
                    ui.label(if running {
                        text.color(Color32::WHITE)
                    } else {
                        text
                    });
                });
        });
    });
}

fn move_list(ui: &mut egui::Ui, game: &Game) {
    let moves = game.san_moves();
    if moves.is_empty() {
        ui.label(RichText::new("No moves yet").weak());
        return;
    }
    let mono = FontId::monospace(14.0);
    egui::Grid::new(ui.next_auto_id())
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
