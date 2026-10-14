//! The native menu bar. Menu clicks become `Action`s, handled like any other
//! UI input; macOS itself draws the menus and handles the ⌘ shortcuts.

use crate::Action;
use crate::assets::{BOARDS, PIECE_SETS};
use eframe::egui;
use muda::accelerator::Accelerator;
use muda::{AboutMetadata, CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use std::sync::mpsc;

/// ⇧⌘D for dark mode.
const DARK_KEY: &str = "Shift+CmdOrCtrl+D";

/// ⌃⌘S, the standard macOS sidebar toggle.
const SIDEBAR_KEY: &str = "Control+Super+S";

/// What the menu currently shows; re-synced only when it changes.
#[derive(Clone, PartialEq, Default)]
pub struct MenuState {
    pub logged_in: bool,
    pub busy: bool,
    pub piece_set: Option<String>,
    pub board: Option<String>,
    pub sidebar_hidden: bool,
    pub dark: bool,
    pub muted: bool,
}

pub struct MenuBar {
    _menu: Menu, // the menu lives as long as this handle
    login: MenuItem,
    logout: MenuItem,
    sidebar: MenuItem,
    light: CheckMenuItem,
    dark: CheckMenuItem,
    sound: CheckMenuItem,
    pieces: Vec<CheckMenuItem>,
    boards: Vec<CheckMenuItem>,
    rx: mpsc::Receiver<String>,
    shown: Option<MenuState>,
}

impl MenuBar {
    /// Must run on the main thread once the app is running (eframe's creation
    /// callback is the right place).
    pub fn install(ctx: &egui::Context) -> Self {
        let key = |s: &str| s.parse::<Accelerator>().ok();
        let item = |id: &str, text: &str, accel: Option<&str>| {
            MenuItem::with_id(id, text, true, accel.and_then(key))
        };

        let about = AboutMetadata {
            name: Some("Caturankam".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            comments: Some(
                "Chess, the way it began.\nA lightweight client for Lichess. Not an official Lichess app."
                    .into(),
            ),
            copyright: Some("© 2026 deepakvettickal · MIT licence".into()),
            ..Default::default()
        };
        let app = submenu(
            "Caturankam",
            &[
                &PredefinedMenuItem::about(None, Some(about)),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(None),
            ],
        );

        let game = submenu(
            "Game",
            &[
                &item("new", "New Local Game", Some("CmdOrCtrl+N")),
                &item("flip", "Flip Board", Some("CmdOrCtrl+F")),
            ],
        );

        let login = item("login", "Log In with Lichess…", None);
        let logout = item("logout", "Log Out", None);
        let account = submenu("Account", &[&login, &logout]);

        let check = |id: String, text: &str| CheckMenuItem::with_id(id, text, true, false, None);
        let pieces: Vec<CheckMenuItem> =
            std::iter::once(check("piece:".into(), "cburnett (built in)"))
                .chain(
                    PIECE_SETS[1..]
                        .iter()
                        .map(|s| check(format!("piece:{s}"), s)),
                )
                .collect();
        let boards: Vec<CheckMenuItem> =
            std::iter::once(check("board:".into(), "stone (built in)"))
                .chain(
                    BOARDS[1..]
                        .iter()
                        .map(|(n, _)| check(format!("board:{n}"), n)),
                )
                .collect();
        let sidebar = item("sidebar", "Hide Sidebar", Some(SIDEBAR_KEY));
        let light = CheckMenuItem::with_id("light", "Light", true, false, None);
        let dark = CheckMenuItem::with_id("dark", "Dark", true, false, key(DARK_KEY));
        let sound = CheckMenuItem::with_id("sound", "Sound", true, true, None);
        let view = submenu(
            "View",
            &[
                &sidebar,
                &submenu("Appearance", &[&light, &dark]),
                &sound,
                &PredefinedMenuItem::separator(),
                &submenu("Pieces", &pieces.iter().map(|i| i as _).collect::<Vec<_>>()),
                &submenu("Board", &boards.iter().map(|i| i as _).collect::<Vec<_>>()),
                &item("random", "Randomize Pieces and Board", Some("CmdOrCtrl+R")),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::fullscreen(None),
            ],
        );

        let window = submenu(
            "Window",
            &[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::maximize(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::bring_all_to_front(None),
            ],
        );
        window.set_as_windows_menu_for_nsapp();

        let menu = Menu::new();
        for sub in [&app, &game, &account, &view, &window] {
            menu.append(sub).expect("append top-level menu");
        }
        menu.init_for_nsapp();

        // Menu clicks arrive on the main thread outside egui's frame: queue them
        // and wake egui so they're handled on the next frame.
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e.id.0);
            ctx.request_repaint();
        }));

        Self {
            _menu: menu,
            login,
            logout,
            sidebar,
            light,
            dark,
            sound,
            pieces,
            boards,
            rx,
            shown: None,
        }
    }

    pub fn actions(&self) -> Vec<Action> {
        self.rx.try_iter().filter_map(|id| action(&id)).collect()
    }

    /// Updates enabled/checked states. Cheap when nothing changed.
    pub fn sync(&mut self, state: MenuState) {
        if self.shown.as_ref() == Some(&state) {
            return;
        }
        self.login.set_enabled(!state.logged_in && !state.busy);
        self.logout.set_enabled(state.logged_in);
        self.sidebar.set_text(if state.sidebar_hidden {
            "Show Sidebar"
        } else {
            "Hide Sidebar"
        });
        self.light.set_checked(!state.dark);
        self.dark.set_checked(state.dark);
        self.sound.set_checked(!state.muted);
        let piece = format!("piece:{}", state.piece_set.as_deref().unwrap_or(""));
        for i in &self.pieces {
            i.set_checked(i.id().0 == piece);
        }
        let board = format!("board:{}", state.board.as_deref().unwrap_or(""));
        for i in &self.boards {
            i.set_checked(i.id().0 == board);
        }
        self.shown = Some(state);
    }
}

fn submenu(text: &str, items: &[&dyn muda::IsMenuItem]) -> Submenu {
    Submenu::with_items(text, true, items).expect("build submenu")
}

/// Maps a menu item id to what it does. Empty names mean "built in".
fn action(id: &str) -> Option<Action> {
    let name = |s: &str| (!s.is_empty()).then(|| s.to_string());
    Some(match id {
        "new" => Action::NewGame,
        "flip" => Action::Flip,
        "login" => Action::Login,
        "logout" => Action::Logout,
        "random" => Action::Randomize,
        "sidebar" => Action::ToggleSidebar,
        "light" => Action::Dark(false),
        "dark" => Action::Dark(true),
        "sound" => Action::ToggleSound,
        _ => {
            if let Some(set) = id.strip_prefix("piece:") {
                Action::Pieces(name(set))
            } else {
                Action::Board(name(id.strip_prefix("board:")?))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_map_to_actions() {
        assert_eq!(action("flip"), Some(Action::Flip));
        assert_eq!(action("piece:"), Some(Action::Pieces(None)));
        assert_eq!(
            action("piece:fantasy"),
            Some(Action::Pieces(Some("fantasy".into())))
        );
        assert_eq!(
            action("board:wood4"),
            Some(Action::Board(Some("wood4".into())))
        );
        assert_eq!(action("nonsense"), None);
        assert_eq!(action("dark"), Some(Action::Dark(true)));
        assert!(SIDEBAR_KEY.parse::<Accelerator>().is_ok());
        assert!(DARK_KEY.parse::<Accelerator>().is_ok());
    }
}
