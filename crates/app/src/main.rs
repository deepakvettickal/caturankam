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
mod sound;
mod style;
mod tv;

use assets::{Assets, BOARDS, Fetch, PIECE_SETS, PieceSet};
use board::{BoardView, Look, piece_image};
use caturankam_core::{Color, Game, KnownOutcome, Outcome, Piece, Role, Square};
use caturankam_lichess::{GameEvent, Side, TvEvent};
use caturankam_theme::Theme;
use eframe::egui::{self, Color32, Key, Margin, Modifiers, RichText, Stroke, Vec2};
use net::{Cmd, Evt, Net};
use online::{OnlineGame, format_clock, next_tick, player_label};
use settings::Settings;
use std::time::{Duration, Instant};
use style::{Kind, Pal, button, kbd, medium, mono, muted, row, rule, section};
use tv::TvGame;

/// Rapid and Classical: the only real-time controls Lichess lets Board API
/// clients seek. (minutes, increment), on keys 1–5.
const SEEKS: [(u32, u32); 5] = [(10, 0), (10, 5), (15, 10), (30, 0), (30, 20)];
/// Against the computer Blitz is allowed too. Keys 6–0 and -.
const AI_CLOCKS: [(u32, u32); 6] = [(3, 2), (5, 0), (5, 3), (10, 0), (10, 5), (15, 10)];
const SEEK_KEYS: [&str; 5] = ["1", "2", "3", "4", "5"];
const AI_KEYS: [&str; 6] = ["6", "7", "8", "9", "0", "-"];
/// Challenging a friend allows Blitz too (Bullet isn't playable through the
/// Board API). The picker starts on 5+3.
const FRIEND_CLOCKS: [(u32, u32); 8] = [
    (3, 0),
    (3, 2),
    (5, 0),
    (5, 3),
    (10, 0),
    (10, 5),
    (15, 10),
    (30, 0),
];
const NOTICE_SECS: f64 = 6.0;
/// After hiding the sidebar, "Show sidebar [S]" stays this long, then fades.
const HINT_SECS: f64 = 2.0;
const HINT_FADE_SECS: f64 = 0.6;
/// Space between the board and its crop frame.
const FRAME: f32 = 3.0;

fn main() -> eframe::Result {
    // For packaging smoke tests: proves the bundled binary runs, without a window.
    if std::env::args().any(|a| a == "--version") {
        println!("caturankam {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let viewport = egui::ViewportBuilder::default()
        .with_title("Caturankam")
        .with_inner_size([1040.0, 760.0])
        .with_min_inner_size([720.0, 480.0])
        .with_icon(dock_icon());
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

/// The Dock icon set at runtime. Left empty, eframe would put egui's logo
/// there, over the bundle's own icon; empty `IconData` means "don't set
/// one", so the `.app` shows its `.icns`, sharp at every size. Dev builds
/// (`cargo run`) have no bundle, so they set ours; release builds don't
/// carry the image.
fn dock_icon() -> egui::IconData {
    #[cfg(debug_assertions)]
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/macos/icon-1024.png"))
    {
        return icon;
    }
    egui::IconData::default()
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
    ToggleSound,
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
    /// Y: accept a draw offer or the incoming challenge.
    Accept,
    ClaimVictory,
    /// U: focus the friend's username field.
    FocusFriend,
    /// Send the challenge to the friend in the username field.
    ChallengeFriend,
    /// Step through the game's history: one move back or forward, to the
    /// start, back to live, or to the position after a given ply.
    Back,
    Forward,
    First,
    Live,
    View(usize),
    /// W: start or stop watching Lichess TV.
    Watch,
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
    /// Waiting for a game: a seek, or a challenge to a friend.
    Waiting {
        label: String,
        /// When the seek or challenge closed; the game should follow shortly.
        ended_at: Option<f64>,
        /// For a challenge: who, and its id once Lichess has one.
        challenge: Option<(String, Option<String>)>,
    },
    Online(Box<OnlineGame>),
    /// Following Lichess TV; `None` until the first game arrives.
    Watching(Option<Box<TvGame>>),
}

struct Lobby {
    rated: bool,
    ai_level: u8,
    ai_color: Option<Side>,
    /// Challenge a friend: their username and an index into `FRIEND_CLOCKS`.
    friend: String,
    friend_clock: usize,
    focus_friend: bool,
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
    /// The ply being looked at while stepping through history; `None` is live.
    viewing: Option<usize>,
    /// Challenges to us, oldest first.
    incoming: Vec<caturankam_lichess::Challenge>,
    sound: sound::Player,
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
                piece_set: d.pieces.clone(),
                board: d.board.clone(),
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
                friend: String::new(),
                friend_clock: 3,
                focus_friend: false,
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
            viewing: None,
            incoming: Vec::new(),
            sound: sound::Player::default(),
        };
        if let Some(d) = &app.demo {
            if let Ok((game, _)) = Game::from_uci("startpos", &d.moves) {
                app.local = game;
            }
            if let Some(user) = &d.user {
                app.account = Account::LoggedIn(user.clone());
            }
            if d.watch {
                app.act(ctx, Action::Watch);
            }
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

    fn play_sound(&mut self, s: sound::Sound) {
        if !self.settings.muted {
            self.sound.play(s);
        }
    }

    fn notify(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        let now = ctx.input(|i| i.time);
        self.notice = Some((msg.into(), now + NOTICE_SECS));
        ctx.request_repaint_after(Duration::from_secs_f64(NOTICE_SECS));
    }

    fn game(&self) -> &Game {
        match &self.play {
            Play::Online(g) => &g.game,
            Play::Watching(Some(g)) => &g.game,
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
        if let Play::Waiting {
            ended_at: Some(t),
            challenge,
            ..
        } = &self.play
        {
            // The seek or challenge closed but no game arrived.
            if ctx.input(|i| i.time) - t > 5.0 {
                let msg = if challenge.is_some() {
                    "The game didn't start. Try again."
                } else {
                    "No opponent found. Try again."
                };
                self.play = Play::Local;
                self.notify(ctx, msg);
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
                if let Play::Waiting { ended_at, .. } = &mut self.play {
                    *ended_at = Some(ctx.input(|i| i.time));
                }
            }
            Evt::ChallengeCreated(id) => {
                if let Play::Waiting {
                    challenge: Some((_, slot)),
                    ..
                } = &mut self.play
                {
                    *slot = Some(id);
                }
            }
            Evt::ChallengeEnded(done) => {
                let now = ctx.input(|i| i.time);
                if let Play::Waiting {
                    challenge: Some((to, _)),
                    ended_at,
                    ..
                } = &mut self.play
                {
                    let to = to.clone();
                    match done.as_str() {
                        // The game itself arrives through the event stream.
                        "accepted" => *ended_at = Some(now),
                        "declined" => {
                            self.play = Play::Local;
                            self.notify(ctx, format!("{to} declined the challenge."));
                        }
                        "failed" => self.play = Play::Local, // the notice is already up
                        _ => {
                            self.play = Play::Local;
                            self.notify(ctx, format!("{to} didn't answer in time."));
                        }
                    }
                }
            }
            Evt::Incoming(ch) => {
                if !self.incoming.iter().any(|c| c.id == ch.id) {
                    self.incoming.push(ch);
                }
            }
            Evt::IncomingGone(id) => self.incoming.retain(|c| c.id != id),
            Evt::Game(ev) => self.on_game_event(ctx, ev),
            Evt::Tv(ev) => self.on_tv_event(ctx, ev),
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
                // An accepted challenge's game has the challenge's id.
                self.incoming.retain(|c| c.id != full.id);
                match OnlineGame::new(*full, &my_id) {
                    Ok(g) => {
                        if !same {
                            // A new game: reset the board and face our side.
                            self.board.reset();
                            self.viewing = None;
                            self.board.flipped = g.me == Color::Black;
                            self.play_sound(sound::Sound::Start);
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
                let (ply, was_over) = (g.game.ply(), g.is_over());
                let result = g.update(&st);
                let (over, my_turn, moved) = (g.is_over(), g.my_turn(), g.game.ply() != ply);
                if over {
                    self.promoting = None;
                    self.board.premove = None;
                }
                if moved {
                    self.viewing = None; // a new move snaps back to the live position
                }
                match result {
                    Ok(Some(ev)) => {
                        self.board.animate(&ev, now);
                        self.play_sound(sound::Sound::for_move(&ev));
                    }
                    Ok(None) => {}
                    Err(e) => self.notify(ctx, format!("Out of sync with Lichess: {e}")),
                }
                if over && !was_over {
                    self.play_sound(sound::Sound::End);
                }
                if my_turn && let Some((from, to)) = self.board.premove.take() {
                    // Played at once, unanimated; dropped if it's no longer legal.
                    let promo = self.game().is_promotion(from, to).then_some(Role::Queen);
                    self.play_move(from, to, promo, false, now);
                }
            }
            GameEvent::OpponentGone {
                gone,
                claim_win_in_seconds,
            } => {
                if let Play::Online(g) = &mut self.play {
                    let wait = Duration::from_secs(claim_win_in_seconds.unwrap_or(0).into());
                    g.opponent_gone = gone.then(|| Instant::now() + wait);
                }
            }
            GameEvent::Other => {}
        }
    }

    /// Lichess TV: a new game replaces the old one (that's how the feed moves
    /// on), and each move animates like an opponent's would.
    fn on_tv_event(&mut self, ctx: &egui::Context, ev: TvEvent) {
        let Play::Watching(watching) = &mut self.play else {
            return; // stopped while this was on its way
        };
        let now = ctx.input(|i| i.time);
        match ev {
            TvEvent::Featured(f) => match TvGame::new(f) {
                Ok(g) => {
                    self.board.reset();
                    self.board.flipped = g.bottom == Color::Black;
                    self.viewing = None;
                    *watching = Some(Box::new(g));
                }
                Err(e) => self.notify(ctx, format!("Couldn't load the TV game: {e}")),
            },
            TvEvent::Fen(m) => {
                let Some(g) = watching else { return };
                match g.update(&m) {
                    Ok(Some(ev)) => {
                        self.viewing = None;
                        self.board.animate(&ev, now);
                        self.play_sound(sound::Sound::for_move(&ev));
                    }
                    Ok(None) => self.viewing = None,
                    Err(e) => self.notify(ctx, format!("Out of sync with Lichess TV: {e}")),
                }
            }
        }
    }

    // ---- actions ----

    fn back_to_local(&mut self) {
        self.viewing = None;
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
        self.viewing = None;
        self.play_sound(sound::Sound::for_move(&ev));
        if matches!(self.play, Play::Local) && self.local.outcome() != Outcome::Unknown {
            self.play_sound(sound::Sound::End);
        }
        if animate {
            self.board.animate(&ev, now);
        }
    }

    /// Cancels a seek or withdraws a challenge, whichever we're waiting on.
    fn cancel_waiting(&mut self, ctx: &egui::Context) {
        if let Play::Waiting { challenge, .. } = &self.play {
            let cmd = match challenge {
                Some((_, id)) => Cmd::CancelChallenge(id.clone()),
                None => Cmd::CancelSeek,
            };
            self.net(ctx).send(cmd);
            self.play = Play::Local;
        }
    }

    /// Incoming challenges are offered only between games.
    fn incoming_shown(&self) -> bool {
        matches!(self.play, Play::Local) && !self.incoming.is_empty()
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
            Action::Back => {
                let ply = self.game().ply();
                self.viewing = Some(self.viewing.unwrap_or(ply).saturating_sub(1));
            }
            Action::Forward => {
                let ply = self.game().ply();
                self.viewing = self.viewing.map(|v| v + 1).filter(|&v| v < ply);
            }
            Action::First => self.viewing = (self.game().ply() > 0).then_some(0),
            Action::Live => self.viewing = None,
            Action::View(p) => self.viewing = (p < self.game().ply()).then_some(p),
            Action::NewGame => {
                if matches!(self.play, Play::Local) {
                    self.viewing = None;
                    self.local = Game::default();
                    self.board.reset();
                    self.promoting = None;
                } else if matches!(self.play, Play::Watching(_)) {
                    self.notify(ctx, "Stop watching first (W).");
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
                self.cancel_waiting(ctx);
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
            Action::ToggleSound => {
                self.settings.muted = !self.settings.muted;
                self.settings.save();
                if !self.settings.muted {
                    self.play_sound(sound::Sound::Move); // a sample, so you hear it's on
                }
            }
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
                self.play = Play::Waiting {
                    label,
                    ended_at: None,
                    challenge: None,
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
            Action::FocusFriend if in_lobby => {
                self.lobby.focus_friend = true;
                if !self.sidebar_visible() {
                    self.act(ctx, Action::ToggleSidebar);
                }
            }
            Action::ChallengeFriend if in_lobby => {
                let to = self.lobby.friend.trim().to_string();
                if !caturankam_lichess::valid_username(&to) {
                    self.notify(ctx, "Lichess usernames are 2–30 letters, digits, - or _.");
                    return;
                }
                let (minutes, increment) = FRIEND_CLOCKS[self.lobby.friend_clock];
                let rated = self.lobby.rated;
                let label = format!(
                    "{minutes}+{increment} {} · {}",
                    speed(minutes, increment),
                    if rated { "rated" } else { "casual" }
                );
                self.play = Play::Waiting {
                    label,
                    ended_at: None,
                    challenge: Some((to.clone(), None)),
                };
                self.net(ctx).send(Cmd::Challenge {
                    username: to,
                    limit_secs: minutes * 60,
                    increment,
                    rated,
                    color: None,
                });
            }
            Action::Accept if draw_offer => self.game_cmd(|game| Cmd::Draw { game, accept: true }),
            Action::Accept if self.incoming_shown() => {
                let ch = self.incoming.remove(0);
                self.net(ctx).send(Cmd::AnswerChallenge {
                    id: ch.id,
                    accept: true,
                });
            }
            Action::ClaimVictory if self.online().is_some_and(|g| g.can_claim_victory()) => {
                self.game_cmd(Cmd::ClaimVictory)
            }
            Action::Escape if self.board.premove.is_some() => self.board.premove = None,
            Action::CancelSeek | Action::Escape if matches!(self.play, Play::Waiting { .. }) => {
                self.cancel_waiting(ctx);
            }
            Action::Escape if self.incoming_shown() => {
                let ch = self.incoming.remove(0);
                self.net(ctx).send(Cmd::AnswerChallenge {
                    id: ch.id,
                    accept: false,
                });
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
            Action::Watch => match self.play {
                // The network thread starts here if nothing else needed it yet.
                Play::Local => {
                    self.net(ctx).send(Cmd::WatchTv);
                    self.play = Play::Watching(None);
                    self.board.reset();
                    self.viewing = None;
                    self.promoting = None;
                }
                Play::Watching(_) => {
                    self.net(ctx).send(Cmd::StopTv);
                    self.back_to_local();
                }
                _ => self.notify(ctx, "Finish the online game first."),
            },
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
            (Key::M, Action::ToggleSound),
            (Key::R, Action::Randomize),
            (Key::L, Action::Login),
            (Key::N, Action::NewGame),
            (Key::OpenBracket, Action::Level(-1)),
            (Key::CloseBracket, Action::Level(1)),
            (Key::C, Action::CycleColor),
            (Key::A, Action::Abort),
            (Key::X, Action::Resign),
            (Key::O, Action::OfferDraw),
            (Key::Y, Action::Accept),
            (Key::V, Action::ClaimVictory),
            (Key::U, Action::FocusFriend),
            (Key::W, Action::Watch),
            (Key::Escape, Action::Escape),
            (Key::ArrowLeft, Action::Back),
            (Key::ArrowRight, Action::Forward),
            (Key::ArrowUp, Action::First),
            (Key::ArrowDown, Action::Live),
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
            Key::Minus,
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
                // Scrolls when the window is too short for the lobby.
                egui::ScrollArea::vertical()
                    .id_salt("sidebar")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        // A little room so glyph edges aren't clipped on the left and
                        // the scrollbar doesn't cover anything on the right.
                        egui::Frame::new()
                            .inner_margin(Margin {
                                left: 2,
                                right: 10,
                                top: 0,
                                bottom: 0,
                            })
                            .show(ui, |ui| {
                                self.account_row(ui);
                                match &self.play {
                                    Play::Local => self.local_panel(ui),
                                    Play::Waiting { .. } => self.waiting_panel(ui),
                                    Play::Online(_) => self.online_panel(ui),
                                    Play::Watching(_) => self.watch_panel(ui),
                                }
                            });
                    });
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
        ui.label(RichText::new(headline).font(medium(14.0)));
        ui.add_space(4.0);
        history_hint(ui, &pal, self.viewing, self.local.ply());
        let current = self.viewing.unwrap_or(self.local.ply());
        let viewed = egui::ScrollArea::vertical()
            .id_salt("local-moves")
            .max_height(120.0)
            .stick_to_bottom(self.viewing.is_none())
            .show(ui, |ui| move_list(ui, &pal, &self.local, current))
            .inner;
        if let Some(p) = viewed {
            self.act(&ctx, Action::View(p));
        }
        rule(ui);

        section(ui, &pal, "Watch live", |ui| {
            if button(ui, &pal, "Watch", Some("W"), Kind::Secondary).clicked() {
                self.act(&ctx, Action::Watch);
            }
        });
        ui.label(muted(
            &pal,
            "The top game on Lichess right now, as on Lichess TV.",
        ));
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
            "Lichess pairs apps like this one at Rapid and Classical only. Blitz works against the computer and friends.",
        ));
        rule(ui);

        section(ui, &pal, "Challenge a friend", |ui| kbd(ui, &pal, "U"));
        ui.horizontal(|ui| {
            // The field takes what the clock picker and Send button leave.
            let width = (ui.available_width() - 160.0).max(80.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.lobby.friend)
                    .hint_text("Username")
                    .desired_width(width)
                    .char_limit(30),
            );
            if std::mem::take(&mut self.lobby.focus_friend) {
                field.request_focus();
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let (m, inc) = FRIEND_CLOCKS[self.lobby.friend_clock];
            egui::ComboBox::from_id_salt("friend-clock")
                .selected_text(RichText::new(format!("{m}+{inc}")).font(mono(13.0)))
                .width(64.0)
                .show_ui(ui, |ui| {
                    for (i, &(m, inc)) in FRIEND_CLOCKS.iter().enumerate() {
                        let text = format!("{m}+{inc}  {}", speed(m, inc));
                        ui.selectable_value(&mut self.lobby.friend_clock, i, text);
                    }
                });
            if button(ui, &pal, "Send", Some("↵"), Kind::Secondary).clicked() || enter {
                self.act(&ctx, Action::ChallengeFriend);
            }
        });
        ui.label(muted(
            &pal,
            "Rated follows Quick pairing; colour is random.",
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

    fn waiting_panel(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let Play::Waiting {
            label,
            ended_at,
            challenge,
        } = &self.play
        else {
            return;
        };
        let (title, text) = match (challenge, ended_at) {
            (_, Some(_)) => ("Starting", "Starting game".to_string()),
            (Some((to, _)), None) => ("Challenge", format!("Waiting for {to} to accept")),
            (None, None) => ("Quick pairing", "Finding an opponent".to_string()),
        };
        section(ui, &pal, title, |_| {});
        waiting(ui, &text);
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
        if let Some(at) = g.opponent_gone.filter(|_| !g.is_over()) {
            let left = at.saturating_duration_since(Instant::now());
            if left.is_zero() {
                ui.horizontal(|ui| {
                    ui.label(muted(&pal, "Your opponent left."));
                    if button(ui, &pal, "Claim victory", Some("V"), Kind::Primary).clicked() {
                        action = Some(Action::ClaimVictory);
                    }
                });
            } else {
                ui.label(muted(
                    &pal,
                    format!(
                        "Opponent left. You can claim victory in {}s.",
                        left.as_secs() + 1
                    ),
                ));
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
        }
        ui.add_space(6.0);
        history_hint(ui, &pal, self.viewing, g.game.ply());
        let current = self.viewing.unwrap_or(g.game.ply());
        let viewed = egui::ScrollArea::vertical()
            .id_salt("online-moves")
            .max_height((ui.available_height() - 150.0).max(60.0))
            .stick_to_bottom(self.viewing.is_none())
            .show(ui, |ui| move_list(ui, &pal, &g.game, current))
            .inner;
        if let Some(p) = viewed {
            action = Some(Action::View(p));
        }
        ui.add_space(6.0);
        player_card(ui, &pal, g, g.me, now);
        rule(ui);

        if g.is_over() {
            ui.label(RichText::new(g.result()).font(medium(18.0)));
            ui.add_space(8.0);
            if button(ui, &pal, "Back to lobby", Some("↵"), Kind::Primary).clicked() {
                action = Some(Action::BackToLobby);
            }
            if let Some(a) = action {
                self.act(&ctx, a);
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

    /// Lichess TV: both players, their titles, ratings and clocks, the top
    /// player first as on the board.
    fn watch_panel(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        section(ui, &pal, "Watch live", |ui| {
            if button(ui, &pal, "Stop", Some("W"), Kind::Secondary).clicked() {
                self.act(&ctx, Action::Watch);
            }
        });
        let Play::Watching(Some(g)) = &self.play else {
            waiting(ui, "Tuning in to Lichess TV");
            return;
        };
        let now = Instant::now();
        let top = if self.board.flipped {
            Color::White
        } else {
            Color::Black
        };
        let tv_row = |ui: &mut egui::Ui, c: Color| {
            let p = g.player(c);
            let name = match (&p.user, p.ai) {
                (Some(u), _) => u.name.clone(),
                (None, Some(level)) => format!("Stockfish level {level}"),
                _ => "Anonymous".into(),
            };
            let title = p.user.as_ref().and_then(|u| u.title.as_deref());
            let running = g.ticking() && g.game.turn() == c;
            let clock = g.clock(c, now).map(|left| (left, running));
            let lead = g.game.material_lead(c);
            player_row(ui, &pal, &name, title, p.rating, lead, clock);
        };
        tv_row(ui, top);
        ui.add_space(4.0);
        let status = match g.game.outcome() {
            Outcome::Known(KnownOutcome::Decisive { .. }) => "Checkmate · the next game follows",
            Outcome::Known(KnownOutcome::Draw) => "Draw · the next game follows",
            Outcome::Unknown if g.game.turn() == Color::White => "White to move",
            Outcome::Unknown => "Black to move",
        };
        ui.label(muted(&pal, status));
        ui.add_space(4.0);
        tv_row(ui, !top);
        ui.add_space(8.0);
        ui.label(muted(
            &pal,
            "When this game ends, the next one starts here on its own.",
        ));
    }

    /// The board on a hatched field, inside a crop-mark frame. It always gets
    /// the full height: names and clocks live in the sidebar or window title.
    fn board_area(&mut self, ui: &mut egui::Ui, now: f64) {
        let pal = self.pal;
        let field = ui.max_rect();
        style::hatch(ui.painter(), field, pal.line);
        self.show_sidebar_hint(ui);
        self.incoming_card(ui);

        // Just enough room for the frame and its crop-mark overshoot.
        let area = field.shrink(FRAME + style::CROP + 2.0);
        let card = board::board_rect(area).expand(FRAME);
        ui.painter().rect_filled(card, 0.0, pal.bg);
        style::crop_frame(ui.painter(), card, pal.line.gamma_multiply(1.6).to_opaque());

        let movable = match &self.play {
            _ if self.promoting.is_some() || self.viewing.is_some() => None,
            Play::Local => (self.local.outcome() == Outcome::Unknown).then(|| self.local.turn()),
            Play::Waiting { .. } | Play::Watching(_) => None,
            // Always our side in a live game: on the opponent's turn it's a premove.
            Play::Online(g) => (!g.is_over()).then_some(g.me),
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
        // Matching on the field (not `self.game()`) borrows only `self.play`,
        // leaving `self.board` free for the closure below.
        let live = match &self.play {
            Play::Online(g) => &g.game,
            Play::Watching(Some(g)) => &g.game,
            _ => &self.local,
        };
        // Looking back: show the position then (a rebuilt copy; cheap).
        let past = self.viewing.map(|p| live.at(p));
        let game = past.as_ref().unwrap_or(live);
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

    /// The oldest challenge to us, as a card over the board: Y accepts, Esc declines.
    fn incoming_card(&mut self, ui: &mut egui::Ui) {
        if !self.incoming_shown() {
            return;
        }
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let ch = &self.incoming[0];
        let who = match (&ch.challenger.title, ch.challenger.rating) {
            (Some(t), Some(r)) => format!("{t} {} ({r})", ch.challenger.name),
            (None, Some(r)) => format!("{} ({r})", ch.challenger.name),
            (Some(t), None) => format!("{t} {}", ch.challenger.name),
            (None, None) => ch.challenger.name.clone(),
        };
        let clock = match ch.time_control {
            caturankam_lichess::TimeControl::Clock { limit, increment } => {
                format!("{}+{increment}", limit / 60)
            }
            _ => String::new(),
        };
        let kind = if ch.rated { "rated" } else { "casual" };
        let more = self.incoming.len() - 1;
        let mut answer = None;
        egui::Area::new("incoming".into())
            .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 16.0))
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(pal.surface)
                    .stroke(Stroke::new(1.0, pal.line))
                    .corner_radius(3)
                    .inner_margin(Margin::same(14))
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: Color32::from_black_alpha(30),
                    })
                    .show(ui, |ui| {
                        ui.label(RichText::new(format!("{who} challenges you")).font(medium(14.0)));
                        let detail = format!("{clock} {} · {kind}", ch.speed);
                        ui.label(RichText::new(detail).font(mono(12.0)).color(pal.muted));
                        if more > 0 {
                            ui.label(muted(&pal, format!("+{more} more waiting")));
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if button(ui, &pal, "Accept", Some("Y"), Kind::Primary).clicked() {
                                answer = Some(Action::Accept);
                            }
                            if button(ui, &pal, "Decline", Some("esc"), Kind::Secondary).clicked() {
                                answer = Some(Action::Escape);
                            }
                        });
                    });
            });
        if let Some(a) = answer {
            self.act(&ctx, a);
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
        if let Play::Watching(Some(g)) = &self.play
            && g.ticking()
        {
            let left = g.clock(g.game.turn(), now).unwrap_or_default();
            ctx.request_repaint_after(next_tick(left));
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
                muted: self.settings.muted,
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
            // Watching: wait for a move, which brings the clocks.
            let tuned_in = match &self.play {
                Play::Watching(g) => g.as_ref().is_some_and(|g| g.ticking()),
                _ => true,
            };
            let ready = !self.assets.downloading() && tuned_in;
            d.frame(&ctx, ready);
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
        Outcome::Unknown if game.ply() == 0 => (
            format!("Move a piece to begin – {} to move", side(game.turn())),
            "Local game",
        ),
        Outcome::Unknown => (format!("{} to move", side(game.turn())), "Local game"),
    }
}

/// A player's name, rating and clock. The clock is dark while running, red in
/// the last ten seconds.
fn player_card(ui: &mut egui::Ui, pal: &Pal, g: &OnlineGame, c: Color, now: Instant) {
    let p = g.player(c);
    let name = p.name.clone().unwrap_or_else(|| player_label(p));
    let rating = p.rating.filter(|_| p.ai_level.is_none());
    let running = g.ticking() && g.game.turn() == c;
    let clock = g.clock(c, now).map(|left| (left, running));
    let lead = g.game.material_lead(c);
    player_row(ui, pal, &name, p.title.as_deref(), rating, lead, clock);
}

/// Title (GM, IM…), name, rating, material lead and clock (with whether it's
/// running), as Lichess shows a player.
fn player_row(
    ui: &mut egui::Ui,
    pal: &Pal,
    name: &str,
    title: Option<&str>,
    rating: Option<u32>,
    lead: i32,
    clock: Option<(Duration, bool)>,
) {
    ui.horizontal(|ui| {
        if let Some(t) = title {
            // Lichess's title amber; readable on light and dark.
            let amber = Color32::from_rgb(191, 129, 29);
            ui.label(RichText::new(t).font(medium(14.0)).color(amber));
        }
        ui.label(RichText::new(name).font(medium(14.0)));
        if let Some(r) = rating {
            ui.label(
                RichText::new(r.to_string())
                    .font(mono(12.0))
                    .color(pal.muted),
            );
        }
        if lead > 0 {
            ui.label(RichText::new(format!("+{lead}")).font(mono(12.0)));
        }
        let Some((left, running)) = clock else { return };
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
    } else if g.can_claim_victory() {
        title.push_str("   ·   Opponent left: V claims victory");
    }
    title
}

/// The moves in SAN, two per row. The move leading to `current` (a ply) is
/// highlighted; clicking a move returns the ply to view.
fn move_list(ui: &mut egui::Ui, pal: &Pal, game: &Game, current: usize) -> Option<usize> {
    let moves = game.san_moves();
    if moves.is_empty() {
        ui.label(muted(pal, "No moves yet"));
        return None;
    }
    let mut clicked = None;
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
                for (i, san) in pair.iter().enumerate() {
                    let ply = n * 2 + i + 1;
                    let mut text = RichText::new(san).font(mono(13.0));
                    if ply == current {
                        text = text
                            .background_color(pal.accent)
                            .color(style::ink_on(pal.accent));
                    }
                    let label = egui::Label::new(text).sense(egui::Sense::click());
                    if ui
                        .add(label)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        clicked = Some(ply);
                    }
                }
                ui.end_row();
            }
        });
    clicked
}

/// "Viewing move 12 of 30" with the keys to move about, while looking back.
fn history_hint(ui: &mut egui::Ui, pal: &Pal, viewing: Option<usize>, ply: usize) {
    if let Some(v) = viewing {
        ui.horizontal(|ui| {
            ui.label(muted(pal, format!("Viewing move {v} of {ply}")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(muted(pal, "live"));
                kbd(ui, pal, "↓");
                ui.label(muted(pal, "step"));
                kbd(ui, pal, "→");
                kbd(ui, pal, "←");
            });
        });
    }
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
