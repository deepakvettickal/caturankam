mod assets;
mod board;
mod demo;
#[cfg(target_os = "macos")]
mod mac_crash_guard;
#[cfg(target_os = "macos")]
mod menu;
mod net;
mod online;
mod settings;
mod style;

use assets::{Assets, BOARDS, Fetch, PIECE_SETS, PieceSet};
use board::{BoardView, Look, piece_image};
use caturankam_core::{Color, Game, KnownOutcome, Outcome, Piece, Role, Square};
use caturankam_lichess::{GameEvent, Side};
use caturankam_theme::Theme;
use eframe::egui::{self, Color32, Key, Margin, Modifiers, RichText, Stroke, Vec2};
use net::{Cmd, Evt, Net};
use online::{OnlineGame, format_clock, next_tick, player_label};
use settings::Settings;
use std::time::{Duration, Instant};
use style::{Kind, Pal, button, kbd, medium, mono, muted, row, rule, section};

/// Rapid and Classical: the only real-time controls Lichess lets Board API
/// clients seek. (minutes, increment), on keys 1–5.
const SEEKS: [(u32, u32); 5] = [(10, 0), (10, 5), (15, 10), (30, 0), (30, 20)];
/// Against the computer Blitz is allowed too. Keys 6–0.
const AI_CLOCKS: [(u32, u32); 5] = [(3, 2), (5, 3), (10, 0), (10, 5), (15, 10)];
const SEEK_KEYS: [&str; 5] = ["1", "2", "3", "4", "5"];
const AI_KEYS: [&str; 5] = ["6", "7", "8", "9", "0"];
const NOTICE_SECS: f64 = 6.0;
/// After hiding the sidebar, "Show sidebar [S]" stays this long, then fades.
const HINT_SECS: f64 = 2.0;
const HINT_FADE_SECS: f64 = 0.6;
/// Space between the board and its crop frame.
const FRAME: f32 = 3.0;

fn main() -> eframe::Result {
    let viewport = egui::ViewportBuilder::default()
        .with_title("Caturankam")
        .with_inner_size([1040.0, 760.0])
        .with_min_inner_size([720.0, 480.0]);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Caturankam",
        options,
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}

/// Something the user asked for: from a button, a key, or the menu bar.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    NewGame,
    Flip,
    Login,
    Logout,
    /// `None` is the built-in set.
    Pieces(Option<String>),
    /// `None` is the theme's own colours.
    Board(Option<String>),
    Randomize,
    ToggleSidebar,
    /// Light (`false`) or dark (`true`) chrome.
    Dark(bool),
    ToggleDark,
    /// Index into `SEEKS`.
    Seek(usize),
    CancelSeek,
    /// Index into `AI_CLOCKS`.
    Computer(usize),
    Level(i8),
    CycleColor,
    Abort,
    /// First press asks to confirm, second resigns.
    Resign,
    OfferDraw,
    AnswerDraw(bool),
    /// Esc: decline a draw, cancel a seek or a pending resign.
    Escape,
    BackToLobby,
}

enum Account {
    LoggedOut,
    LoggingIn,
    Connecting(String),
    /// Saved session, but Lichess couldn't be reached.
    Offline(String),
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
    pal: Pal,
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
    #[cfg(target_os = "macos")]
    menu: menu::MenuBar,
    confirm_resign: bool,
    draw_offered: bool,
    demo: Option<demo::Demo>,
    /// When the sidebar was last hidden, for the fading "Show sidebar" hint.
    sidebar_hidden_at: Option<f64>,
    /// The window title last sent, so it's only updated when it changes.
    title: String,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        // The window exists by now, so winit's view class can be patched.
        #[cfg(target_os = "macos")]
        mac_crash_guard::install();
        egui_extras::install_image_loaders(ctx);
        style::install_fonts(ctx);
        let theme = Theme::original();
        let demo = demo::Demo::from_env();
        let settings = match &demo {
            Some(d) => Settings {
                dark: d.dark,
                ephemeral: true,
                ..Settings::default()
            },
            None => Settings::load(),
        };
        let pal = Pal::new(&theme, settings.dark);
        let mut app = Self {
            builtin: PieceSet::builtin(&theme),
            theme,
            pal,
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
            #[cfg(target_os = "macos")]
            menu: menu::MenuBar::install(ctx),
            confirm_resign: false,
            draw_offered: false,
            demo,
            sidebar_hidden_at: None,
            title: String::new(),
        };
        if let Some(d) = &app.demo
            && let Ok((game, _)) = Game::from_uci("startpos", &d.moves)
        {
            app.local = game;
        }
        app.apply_appearance(ctx);
        app.apply_mode(ctx);
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

    /// Light or dark: egui's widgets, our palette, and the native window chrome.
    fn apply_mode(&mut self, ctx: &egui::Context) {
        self.pal = Pal::new(&self.theme, self.settings.dark);
        style::apply(ctx, &self.pal);
        let native = if self.pal.dark {
            egui::SystemTheme::Dark
        } else {
            egui::SystemTheme::Light
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(native));
    }

    fn notify(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        let now = ctx.input(|i| i.time);
        self.notice = Some((msg.into(), now + NOTICE_SECS));
        ctx.request_repaint_after(Duration::from_secs_f64(NOTICE_SECS));
    }

    fn game(&self) -> &Game {
        match &self.play {
            Play::Online(g) => &g.game,
            _ => &self.local,
        }
    }

    fn online(&self) -> Option<&OnlineGame> {
        match &self.play {
            Play::Online(g) => Some(g),
            _ => None,
        }
    }

    /// The user's choice, except that a finished game always shows the
    /// sidebar: that's where the result and "Back to lobby" are.
    fn sidebar_visible(&self) -> bool {
        !self.settings.sidebar_hidden || self.online().is_some_and(|g| g.is_over())
    }

    fn username(&self) -> Option<&str> {
        match &self.account {
            Account::LoggedIn(u) => Some(u),
            _ => None,
        }
    }

    fn piece_set(&self) -> &PieceSet {
        self.assets
            .pieces
            .as_ref()
            .map_or(&self.builtin, |(_, s)| s)
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
                ctx.request_repaint_after(Duration::from_millis(500));
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
                match &self.account {
                    Account::LoggingIn => self.account = Account::LoggedOut,
                    Account::Connecting(user) => self.account = Account::Offline(user.clone()),
                    _ => {}
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
                let same = self.online().is_some_and(|g| g.id == full.id);
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

    /// Sends a game command for the current online game, if there is one.
    fn game_cmd(&self, make: impl FnOnce(String) -> Cmd) {
        if let (Some(g), Some(net)) = (self.online(), &self.net) {
            net.send(make(g.id.clone()));
        }
    }

    fn act(&mut self, ctx: &egui::Context, action: Action) {
        let in_lobby = matches!(self.play, Play::Local) && self.username().is_some();
        let live = self.online().filter(|g| !g.is_over());
        let (can_abort, draw_offer) =
            live.map_or((false, false), |g| (g.can_abort(), g.opponent_offers_draw));
        let live = live.is_some();
        match action {
            Action::NewGame => {
                if matches!(self.play, Play::Local) {
                    self.local = Game::default();
                    self.board.reset();
                    self.promoting = None;
                } else {
                    self.notify(ctx, "Finish or leave the online game first.");
                }
            }
            Action::Flip => self.board.flipped = !self.board.flipped,
            Action::Login => {
                if matches!(self.account, Account::LoggedOut) {
                    self.account = Account::LoggingIn;
                    self.net(ctx).send(Cmd::Login);
                }
            }
            Action::Logout => {
                if let Play::Seeking { .. } = self.play {
                    self.net(ctx).send(Cmd::CancelSeek);
                }
                self.net(ctx).send(Cmd::Logout);
            }
            Action::Pieces(set) => {
                self.settings.piece_set = set;
                self.settings.save();
                self.apply_appearance(ctx);
            }
            Action::Board(board) => {
                self.settings.board = board;
                self.settings.save();
                self.apply_appearance(ctx);
            }
            Action::Dark(dark) => {
                self.settings.dark = dark;
                self.settings.save();
                self.apply_mode(ctx);
            }
            Action::ToggleDark => self.act(ctx, Action::Dark(!self.settings.dark)),
            Action::ToggleSidebar => {
                // Hide if it is showing now, show if it is hidden.
                self.settings.sidebar_hidden = self.sidebar_visible();
                self.settings.save();
                self.sidebar_hidden_at =
                    self.settings.sidebar_hidden.then(|| ctx.input(|i| i.time));
            }
            Action::Randomize => {
                // Index 0 of each catalog is the built-in one, which is `None`.
                let pick = |names: Vec<&str>, current: Option<&str>| {
                    let at = names
                        .iter()
                        .position(|n| Some(*n) == current)
                        .map_or(0, |i| i + 1);
                    let i = pick_other(at, names.len() + 1, random());
                    (i > 0).then(|| names[i - 1].to_string())
                };
                self.settings.piece_set =
                    pick(PIECE_SETS[1..].to_vec(), self.settings.piece_set.as_deref());
                let boards = BOARDS[1..].iter().map(|(n, _)| *n).collect();
                self.settings.board = pick(boards, self.settings.board.as_deref());
                self.settings.save();
                self.apply_appearance(ctx);
            }
            Action::Seek(i) if in_lobby => {
                let (minutes, increment) = SEEKS[i];
                let rated = self.lobby.rated;
                let label = format!(
                    "{minutes}+{increment} {} · {}",
                    speed(minutes, increment),
                    if rated { "rated" } else { "casual" }
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
            Action::Computer(i) if in_lobby => {
                let (m, inc) = AI_CLOCKS[i];
                let (level, color) = (self.lobby.ai_level, self.lobby.ai_color);
                self.net(ctx).send(Cmd::ChallengeAi {
                    level,
                    limit_secs: m * 60,
                    increment: inc,
                    color,
                });
            }
            Action::Level(d) if in_lobby => {
                self.lobby.ai_level = (self.lobby.ai_level as i8 + d).clamp(1, 8) as u8;
            }
            Action::CycleColor if in_lobby => {
                self.lobby.ai_color = match self.lobby.ai_color {
                    None => Some(Side::White),
                    Some(Side::White) => Some(Side::Black),
                    Some(Side::Black) => None,
                };
            }
            Action::CancelSeek | Action::Escape if matches!(self.play, Play::Seeking { .. }) => {
                if let Some(net) = &self.net {
                    net.send(Cmd::CancelSeek);
                }
                self.play = Play::Local;
            }
            Action::Abort if can_abort => self.game_cmd(Cmd::Abort),
            Action::Resign if live && !can_abort => {
                if self.confirm_resign {
                    self.game_cmd(Cmd::Resign);
                }
                self.confirm_resign = !self.confirm_resign;
            }
            Action::OfferDraw if live && !can_abort && !self.draw_offered => {
                self.game_cmd(|game| Cmd::Draw { game, accept: true });
                self.draw_offered = true;
            }
            Action::AnswerDraw(accept) if draw_offer => {
                self.game_cmd(|game| Cmd::Draw { game, accept });
            }
            Action::Escape if draw_offer => self.game_cmd(|game| Cmd::Draw {
                game,
                accept: false,
            }),
            Action::Escape => self.confirm_resign = false,
            Action::BackToLobby if self.online().is_some_and(|g| g.is_over()) => {
                self.back_to_local();
            }
            // A key for something that doesn't apply right now.
            _ => {}
        }
    }

    /// Single-key shortcuts, shown as chips on the matching buttons.
    /// Skipped while a text field has focus or the promotion picker is open.
    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || self.promoting.is_some() {
            return;
        }
        let mut bind: Vec<(Key, Action)> = vec![
            (Key::F, Action::Flip),
            (Key::S, Action::ToggleSidebar),
            (Key::T, Action::ToggleDark),
            (Key::R, Action::Randomize),
            (Key::L, Action::Login),
            (Key::N, Action::NewGame),
            (Key::OpenBracket, Action::Level(-1)),
            (Key::CloseBracket, Action::Level(1)),
            (Key::C, Action::CycleColor),
            (Key::A, Action::Abort),
            (Key::X, Action::Resign),
            (Key::O, Action::OfferDraw),
            (Key::Y, Action::AnswerDraw(true)),
            (Key::Escape, Action::Escape),
            (Key::Enter, Action::BackToLobby),
        ];
        let digits = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
            Key::Num0,
        ];
        for (i, k) in digits.into_iter().enumerate() {
            bind.push((
                k,
                if i < 5 {
                    Action::Seek(i)
                } else {
                    Action::Computer(i - 5)
                },
            ));
        }
        for (key, action) in bind {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                self.act(ctx, action);
            }
        }
    }

    // ---- UI ----

    /// Elsewhere (until those platforms get native menus): an in-window bar.
    #[cfg(not(target_os = "macos"))]
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let frame = egui::Frame::new()
            .fill(pal.bg)
            .stroke(Stroke::new(1.0, pal.line))
            .inner_margin(Margin::symmetric(12, 6));
        egui::Panel::top("top")
            .frame(frame)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Caturankam").font(medium(15.0)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ctx = ui.ctx().clone();
                        if let Account::LoggedIn(_) = self.account
                            && button(ui, &pal, "Log out", None, Kind::Secondary).clicked()
                        {
                            self.act(&ctx, Action::Logout);
                        }
                        if button(ui, &pal, "Sidebar", Some("S"), Kind::Secondary).clicked() {
                            self.act(&ctx, Action::ToggleSidebar);
                        }
                        if button(ui, &pal, "Light/Dark", Some("T"), Kind::Secondary).clicked() {
                            self.act(&ctx, Action::ToggleDark);
                        }
                        if button(ui, &pal, "Randomize", Some("R"), Kind::Secondary).clicked() {
                            self.act(&ctx, Action::Randomize);
                        }
                        if button(ui, &pal, "Board & pieces", None, Kind::Secondary).clicked() {
                            self.show_appearance = !self.show_appearance;
                        }
                        if button(ui, &pal, "Flip", Some("F"), Kind::Secondary).clicked() {
                            self.act(&ctx, Action::Flip);
                        }
                    });
                });
            });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let frame = egui::Frame::new()
            .fill(pal.bg)
            .stroke(Stroke::new(1.0, pal.line))
            .inner_margin(Margin::symmetric(18, 16));
        egui::Panel::right("side")
            .frame(frame)
            .exact_size(300.0)
            .resizable(false)
            .show_separator_line(false)
            .show(ui, |ui| {
                let ctx = ui.ctx().clone();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if button(ui, &pal, "Hide sidebar", Some("S"), Kind::Secondary).clicked() {
                        self.act(&ctx, Action::ToggleSidebar);
                    }
                });
                ui.add_space(6.0);
                self.account_row(ui);
                match &self.play {
                    Play::Local => self.local_panel(ui),
                    Play::Seeking { .. } => self.seeking_panel(ui),
                    Play::Online(_) => self.online_panel(ui),
                }
            });
    }

    /// Who's logged in. Logging out lives in the Account menu.
    fn account_row(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        // Clicks are recorded and handled after the match, which borrows `self.account`.
        let (mut cancel, mut reconnect) = (false, None);
        let shown = ui.horizontal(|ui| match &self.account {
            Account::LoggedOut => false,
            Account::LoggingIn => {
                waiting(ui, "Continue in your browser");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    cancel = button(ui, &pal, "Cancel", None, Kind::Secondary).clicked();
                });
                true
            }
            Account::Connecting(user) => {
                waiting(ui, &format!("Connecting as {user}"));
                true
            }
            Account::Offline(user) => {
                ui.label(RichText::new(user).font(medium(13.5)));
                ui.label(muted(&pal, "offline"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, &pal, "Reconnect", None, Kind::Secondary).clicked() {
                        reconnect = Some(user.clone());
                    }
                });
                true
            }
            Account::LoggedIn(user) => {
                // A painted dot: no font glyph needed.
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 3.5, Color32::from_rgb(70, 160, 90));
                ui.label(RichText::new(user).font(medium(13.5)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(muted(&pal, "lichess.org"));
                });
                true
            }
        });
        if cancel {
            self.account = Account::LoggedOut;
        }
        if let Some(user) = reconnect {
            self.account = Account::Connecting(user);
            self.net(&ctx).send(Cmd::Connect);
        }
        if shown.inner {
            rule(ui);
        }
    }

    fn local_panel(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let (headline, label) = local_status(&self.local);
        section(ui, &pal, label, |ui| {
            if button(ui, &pal, "New game", Some("N"), Kind::Secondary).clicked() {
                self.act(&ctx, Action::NewGame);
            }
        });
        ui.label(RichText::new(headline).font(medium(22.0)));
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("local-moves")
            .max_height(120.0)
            .stick_to_bottom(true)
            .show(ui, |ui| move_list(ui, &pal, &self.local));
        rule(ui);

        if self.username().is_none() {
            section(ui, &pal, "Play online", |_| {});
            ui.label(muted(
                &pal,
                "Rated and casual games with your Lichess account.",
            ));
            ui.add_space(8.0);
            let busy = !matches!(self.account, Account::LoggedOut);
            let login = ui.add_enabled_ui(!busy, |ui| {
                button(ui, &pal, "Log in with Lichess", Some("L"), Kind::Primary)
            });
            if login.inner.clicked() {
                self.act(&ctx, Action::Login);
            }
            return;
        }
        self.lobby(ui);
    }

    fn lobby(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();

        section(ui, &pal, "Quick pairing", |ui| {
            ui.checkbox(&mut self.lobby.rated, "Rated");
        });
        for (i, &(m, inc)) in SEEKS.iter().enumerate() {
            if row(
                ui,
                &pal,
                &format!("{m}+{inc}"),
                speed(m, inc),
                Some(SEEK_KEYS[i]),
            )
            .clicked()
            {
                self.act(&ctx, Action::Seek(i));
            }
        }
        ui.add_space(4.0);
        ui.label(muted(
            &pal,
            "Lichess pairs apps like this one at Rapid and Classical only. Blitz works against the computer.",
        ));
        rule(ui);

        section(ui, &pal, "Play the computer", |_| {});
        ui.horizontal(|ui| {
            ui.label(muted(&pal, "Level"));
            if button(ui, &pal, "−", Some("["), Kind::Secondary).clicked() {
                self.act(&ctx, Action::Level(-1));
            }
            ui.label(RichText::new(self.lobby.ai_level.to_string()).font(mono(14.0)));
            if button(ui, &pal, "+", Some("]"), Kind::Secondary).clicked() {
                self.act(&ctx, Action::Level(1));
            }
        });
        ui.horizontal(|ui| {
            ui.label(muted(&pal, "Colour"));
            for (side, label) in [
                (None, "Random"),
                (Some(Side::White), "White"),
                (Some(Side::Black), "Black"),
            ] {
                ui.selectable_value(&mut self.lobby.ai_color, side, label);
            }
            kbd(ui, &pal, "C");
        });
        ui.add_space(4.0);
        for (i, &(m, inc)) in AI_CLOCKS.iter().enumerate() {
            if row(
                ui,
                &pal,
                &format!("{m}+{inc}"),
                speed(m, inc),
                Some(AI_KEYS[i]),
            )
            .clicked()
            {
                self.act(&ctx, Action::Computer(i));
            }
        }
    }

    fn seeking_panel(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let Play::Seeking { label, ended_at } = &self.play else {
            return;
        };
        section(ui, &pal, "Quick pairing", |_| {});
        let text = if ended_at.is_some() {
            "Starting game"
        } else {
            "Finding an opponent"
        };
        waiting(ui, text);
        ui.label(
            RichText::new(label.as_str())
                .font(mono(12.5))
                .color(pal.muted),
        );
        ui.add_space(10.0);
        if button(ui, &pal, "Cancel", Some("esc"), Kind::Secondary).clicked() {
            self.act(&ctx, Action::CancelSeek);
        }
    }

    /// Opponent's card at the top, moves, your card at the bottom, actions.
    fn online_panel(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let Play::Online(g) = &self.play else { return };
        let kind = if g.rated { "rated" } else { "casual" };
        section(ui, &pal, "Game", |ui| {
            ui.label(
                RichText::new(format!("{} · {kind}", g.speed))
                    .font(mono(12.0))
                    .color(pal.muted),
            );
        });
        let now = Instant::now();
        let them = if g.me == Color::White {
            Color::Black
        } else {
            Color::White
        };
        player_card(ui, &pal, g, them, now);
        let mut action = None;
        if g.opponent_offers_draw && !g.is_over() {
            ui.horizontal(|ui| {
                ui.label(muted(&pal, "Draw offered"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, &pal, "Decline", Some("esc"), Kind::Secondary).clicked() {
                        action = Some(Action::AnswerDraw(false));
                    }
                    if button(ui, &pal, "Accept", Some("Y"), Kind::Primary).clicked() {
                        action = Some(Action::AnswerDraw(true));
                    }
                });
            });
        }
        if let Some(secs) = g.opponent_gone {
            ui.label(muted(
                &pal,
                format!("Opponent left. You can claim victory in {secs}s on lichess.org."),
            ));
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("online-moves")
            .max_height((ui.available_height() - 150.0).max(60.0))
            .stick_to_bottom(true)
            .show(ui, |ui| move_list(ui, &pal, &g.game));
        ui.add_space(6.0);
        player_card(ui, &pal, g, g.me, now);
        rule(ui);

        if g.is_over() {
            ui.label(RichText::new(g.result()).font(medium(18.0)));
            ui.add_space(8.0);
            if button(ui, &pal, "Back to lobby", Some("↵"), Kind::Primary).clicked() {
                self.act(&ctx, Action::BackToLobby);
            }
            return;
        }
        let can_abort = g.can_abort();
        ui.horizontal(|ui| {
            if can_abort {
                if button(ui, &pal, "Abort", Some("A"), Kind::Secondary).clicked() {
                    action = Some(Action::Abort);
                }
                return;
            }
            let resign = if self.confirm_resign {
                "Confirm resign"
            } else {
                "Resign"
            };
            if button(ui, &pal, resign, Some("X"), Kind::Secondary).clicked() {
                action = Some(Action::Resign);
            }
            let draw = if self.draw_offered {
                "Draw offered"
            } else {
                "Offer draw"
            };
            let offer = ui.add_enabled_ui(!self.draw_offered, |ui| {
                button(ui, &pal, draw, Some("O"), Kind::Secondary)
            });
            if offer.inner.clicked() {
                action = Some(Action::OfferDraw);
            }
        });
        if let Some(a) = action {
            self.act(&ctx, a);
        }
    }

    /// The board on a hatched field, inside a crop-mark frame. It always gets
    /// the full height: names and clocks live in the sidebar or window title.
    fn board_area(&mut self, ui: &mut egui::Ui, now: f64) {
        let pal = self.pal;
        let field = ui.max_rect();
        style::hatch(ui.painter(), field, pal.line);
        self.show_sidebar_hint(ui);

        // Just enough room for the frame and its crop-mark overshoot.
        let area = field.shrink(FRAME + style::CROP + 2.0);
        let card = board::board_rect(area).expand(FRAME);
        ui.painter().rect_filled(card, 0.0, pal.bg);
        style::crop_frame(ui.painter(), card, pal.line.gamma_multiply(1.6).to_opaque());

        let movable = match &self.play {
            _ if self.promoting.is_some() => None,
            Play::Local => (self.local.outcome() == Outcome::Unknown).then(|| self.local.turn()),
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
        let builder = egui::UiBuilder::new().max_rect(area);
        let intent = ui
            .scope_builder(builder, |ui| self.board.show(ui, game, &look, movable))
            .inner;
        if let Some(i) = intent {
            if self.game().is_promotion(i.from, i.to) {
                self.promoting = Some((i.from, i.to));
            } else {
                self.play_move(i.from, i.to, None, !i.dragged, now);
            }
        }
    }

    /// Right after the sidebar is hidden, a "Show sidebar [S]" button teaches
    /// the way back, then fades out. S and View ▸ Show Sidebar always work.
    fn show_sidebar_hint(&mut self, ui: &mut egui::Ui) {
        let Some(at) = self.sidebar_hidden_at.filter(|_| !self.sidebar_visible()) else {
            return;
        };
        let age = ui.input(|i| i.time) - at;
        let opacity = (1.0 - (age - HINT_SECS) / HINT_FADE_SECS).clamp(0.0, 1.0) as f32;
        if opacity == 0.0 {
            self.sidebar_hidden_at = None;
            return;
        }
        let ctx = ui.ctx().clone();
        // Repaint only while the hint is on screen: at its fade-out start, then per frame.
        if age < HINT_SECS {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(HINT_SECS - age));
        } else {
            ctx.request_repaint();
        }
        let pal = self.pal;
        let clicked = egui::Area::new("show-sidebar".into())
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-12.0, 12.0))
            .show(&ctx, |ui| {
                ui.multiply_opacity(opacity);
                button(ui, &pal, "Show sidebar", Some("S"), Kind::Secondary).clicked()
            })
            .inner;
        if clicked {
            self.act(&ctx, Action::ToggleSidebar);
        }
    }

    /// The window title: "Caturankam", or, in a game with the sidebar hidden,
    /// both players and clocks. Also keeps a running clock repainting.
    fn update_title(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let mut title = String::from("Caturankam");
        if let Some(g) = self.online() {
            if g.ticking() {
                let left = g.clock(g.game.turn(), now).unwrap_or_default();
                ctx.request_repaint_after(next_tick(left));
            }
            if !self.sidebar_visible() {
                title = game_title(g, now);
            }
        }
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Notices float at the bottom of the window, so they show with the
    /// sidebar hidden too.
    fn toast(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let Some((msg, until)) = &self.notice else {
            return;
        };
        if ctx.input(|i| i.time) >= *until {
            self.notice = None;
            return;
        }
        egui::Area::new("toast".into())
            .anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -18.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(pal.surface)
                    .stroke(Stroke::new(1.0, pal.line))
                    .corner_radius(3)
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| ui.label(msg.as_str()));
            });
    }

    /// Non-macOS only; macOS has View ▸ Pieces / Board in the menu bar.
    fn appearance_window(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let mut open = self.show_appearance;
        let (mut changed, mut randomize) = (false, false);
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
                        changed |= ui
                            .selectable_value(
                                &mut self.settings.piece_set,
                                None,
                                "cburnett (built in)",
                            )
                            .changed();
                        for &set in &PIECE_SETS[1..] {
                            changed |= ui
                                .selectable_value(
                                    &mut self.settings.piece_set,
                                    Some(set.into()),
                                    set,
                                )
                                .changed();
                        }
                    });
                egui::ComboBox::from_label("Board")
                    .selected_text(self.settings.board.as_deref().unwrap_or("stone (built in)"))
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(&mut self.settings.board, None, "stone (built in)")
                            .changed();
                        for (name, _) in &BOARDS[1..] {
                            changed |= ui
                                .selectable_value(
                                    &mut self.settings.board,
                                    Some((*name).into()),
                                    *name,
                                )
                                .changed();
                        }
                    });
                ui.add_space(6.0);
                randomize = button(ui, &pal, "Randomize", Some("R"), Kind::Secondary).clicked();
            });
        self.show_appearance = open;
        if changed {
            self.settings.save();
            self.apply_appearance(ctx);
        }
        if randomize {
            self.act(ctx, Action::Randomize);
        }
    }

    fn promotion_picker(&mut self, ctx: &egui::Context, now: f64) {
        let Some((from, to)) = self.promoting else {
            return;
        };
        let pal = self.pal;
        let color = self.game().turn();
        let set = self.piece_set();
        let roles = [
            (Role::Queen, Key::Q, "Q"),
            (Role::Rook, Key::R, "R"),
            (Role::Bishop, Key::B, "B"),
            (Role::Knight, Key::N, "N"),
        ];
        let typed = roles
            .iter()
            .find(|(_, k, _)| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, *k)))
            .map(|(r, _, _)| *r);
        let modal = egui::Modal::new("promotion".into()).show(ctx, |ui| {
            ui.label(RichText::new("Promote to").font(medium(13.5)));
            ui.horizontal(|ui| {
                roles.iter().find_map(|&(role, _, key)| {
                    ui.vertical_centered(|ui| {
                        let img = piece_image(set, Piece { color, role })
                            .fit_to_exact_size(Vec2::splat(56.0));
                        let picked = ui.add(egui::Button::image(img)).clicked();
                        kbd(ui, &pal, key);
                        picked.then_some(role)
                    })
                    .inner
                })
            })
            .inner
        });
        if let Some(role) = typed.or(modal.inner) {
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
        #[cfg(target_os = "macos")]
        {
            for action in self.menu.actions() {
                self.act(&ctx, action);
            }
            self.menu.sync(menu::MenuState {
                logged_in: matches!(self.account, Account::LoggedIn(_) | Account::Offline(_)),
                busy: matches!(self.account, Account::LoggingIn | Account::Connecting(_)),
                piece_set: self.settings.piece_set.clone(),
                board: self.settings.board.clone(),
                sidebar_hidden: !self.sidebar_visible(),
                dark: self.settings.dark,
            });
        }
        self.keys(&ctx);

        #[cfg(not(target_os = "macos"))]
        self.top_bar(ui);
        if self.sidebar_visible() {
            self.side_panel(ui);
        }
        let frame = egui::Frame::new().fill(self.pal.bg);
        egui::CentralPanel::default()
            .frame(frame)
            .show(ui, |ui| self.board_area(ui, now));

        self.update_title(&ctx);
        self.toast(&ctx);
        self.promotion_picker(&ctx, now);
        self.appearance_window(&ctx);
        if let Some(d) = &mut self.demo {
            d.frame(&ctx);
        }
    }
}

/// Lichess's speed categories, by estimated game length (initial + 40 × increment).
fn speed(minutes: u32, increment: u32) -> &'static str {
    match minutes * 60 + increment * 40 {
        0..180 => "Bullet",
        180..480 => "Blitz",
        480..1500 => "Rapid",
        _ => "Classical",
    }
}

/// "Text…" with dots that cycle a few times a second. Unlike a spinner, it
/// repaints at 3 Hz instead of the display's full refresh rate: waits like a
/// browser login can last minutes.
fn waiting(ui: &mut egui::Ui, text: &str) {
    let dots = (ui.input(|i| i.time) * 3.0) as usize % 4;
    ui.label(format!("{text}{:<3}", ".".repeat(dots)));
    ui.ctx().request_repaint_after(Duration::from_millis(333));
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

/// A player's name, rating and clock. The clock is dark while running, red in
/// the last ten seconds.
fn player_card(ui: &mut egui::Ui, pal: &Pal, g: &OnlineGame, c: Color, now: Instant) {
    let p = g.player(c);
    ui.horizontal(|ui| {
        let name = p.name.clone().unwrap_or_else(|| player_label(p));
        ui.label(RichText::new(name).font(medium(14.0)));
        if let Some(r) = p.rating.filter(|_| p.ai_level.is_none()) {
            ui.label(
                RichText::new(r.to_string())
                    .font(mono(12.0))
                    .color(pal.muted),
            );
        }
        let Some(left) = g.clock(c, now) else { return };
        let running = g.ticking() && g.game.turn() == c;
        let red = Color32::from_rgb(190, 45, 35);
        let (fill, ink, border) = match (running, left.as_secs() < 10) {
            (true, true) => (red, style::ink_on(red), red),
            (true, false) => (pal.primary, pal.on_primary, pal.primary),
            _ => (pal.surface, pal.text, pal.line),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::Frame::new()
                .fill(fill)
                .stroke(Stroke::new(1.0, border))
                .corner_radius(3)
                .inner_margin(Margin::symmetric(10, 3))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format_clock(left))
                            .font(mono(20.0))
                            .color(ink),
                    );
                });
        });
    });
}

/// "Stockfish level 3  2:58  ·  the_vetti 1225  ▸ 3:00": opponent first, the
/// running clock marked, a pending draw offer appended.
fn game_title(g: &OnlineGame, now: Instant) -> String {
    let them = if g.me == Color::White {
        Color::Black
    } else {
        Color::White
    };
    let side = |c: Color| {
        let p = g.player(c);
        let name = p.name.clone().unwrap_or_else(|| player_label(p));
        let rating = p
            .rating
            .filter(|_| p.ai_level.is_none())
            .map(|r| format!(" {r}"))
            .unwrap_or_default();
        let clock = g.clock(c, now).map(format_clock).unwrap_or_default();
        let mark = if g.ticking() && g.game.turn() == c {
            "▸ "
        } else {
            ""
        };
        format!("{name}{rating}  {mark}{clock}")
    };
    let mut title = format!("{}   ·   {}", side(them), side(g.me));
    if g.is_over() {
        title = format!("{title}   ·   {}", g.result());
    } else if g.opponent_offers_draw {
        title.push_str("   ·   Draw offered: Y accept, Esc decline");
    }
    title
}

fn move_list(ui: &mut egui::Ui, pal: &Pal, game: &Game) {
    let moves = game.san_moves();
    if moves.is_empty() {
        ui.label(muted(pal, "No moves yet"));
        return;
    }
    egui::Grid::new(ui.next_auto_id())
        .num_columns(3)
        .spacing([18.0, 5.0])
        .show(ui, |ui| {
            for (n, pair) in moves.chunks(2).enumerate() {
                ui.label(
                    RichText::new(format!("{}", n + 1))
                        .font(mono(12.5))
                        .color(pal.muted),
                );
                for san in pair {
                    ui.label(RichText::new(san).font(mono(13.0)));
                }
                ui.end_row();
            }
        });
}

/// A random index in `0..n` other than `current` (when there's a choice).
/// `r` is any random number.
fn pick_other(current: usize, n: usize, r: u64) -> usize {
    if n < 2 {
        return 0;
    }
    let i = (r % (n as u64 - 1)) as usize;
    if i >= current { i + 1 } else { i }
}

/// Good-enough randomness without a dependency: std's hasher keys are
/// randomly seeded per process and per `RandomState`.
fn random() -> u64 {
    use std::hash::BuildHasher;
    std::collections::hash_map::RandomState::new().hash_one(Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_other_never_repeats() {
        for r in 0..100 {
            let i = pick_other(3, 10, r);
            assert!(i < 10 && i != 3);
        }
        assert_eq!(pick_other(0, 1, 7), 0);
        assert_eq!(pick_other(0, 2, 99), 1);
    }

    #[test]
    fn game_title_shows_both_players_and_marks_the_running_clock() {
        let full = r#"{"id":"g","white":{"id":"me","name":"me","rating":1225},"black":{"aiLevel":3},
            "initialFen":"startpos","clock":{"initial":180000,"increment":0},"rated":false,
            "speed":"blitz","state":{"moves":"e2e4 e7e5","wtime":180500,"btime":178000,
            "status":"started","bdraw":true}}"#;
        let g = OnlineGame::new(serde_json::from_str(full).unwrap(), "me").unwrap();
        assert_eq!(
            game_title(&g, Instant::now()),
            "Stockfish level 3  2:58   ·   me 1225  ▸ 3:00   ·   Draw offered: Y accept, Esc decline"
        );
    }

    #[test]
    fn speed_categories_match_lichess() {
        assert_eq!(speed(3, 2), "Blitz");
        assert_eq!(speed(5, 3), "Blitz");
        assert_eq!(speed(10, 0), "Rapid");
        assert_eq!(speed(15, 10), "Rapid");
        assert_eq!(speed(30, 0), "Classical");
        assert_eq!(speed(1, 0), "Bullet");
    }
}
