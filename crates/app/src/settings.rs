//! User settings, saved as TOML in the OS config dir. Never holds secrets:
//! the Lichess token lives in the keychain.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Last logged-in Lichess user; `Some` means "reconnect on startup".
    pub username: Option<String>,
    /// Lichess piece set id; `None` is the built-in set.
    pub piece_set: Option<String>,
    /// Lichess board theme name; `None` is the theme's own colours.
    pub board: Option<String>,
    pub sidebar_hidden: bool,
    pub dark: bool,
    pub muted: bool,
    /// Never written to disk (demo mode for screenshots and benchmarks).
    #[serde(skip)]
    pub ephemeral: bool,
}

impl Settings {
    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("caturankam").join("settings.toml"))
    }

    /// Missing or unreadable settings fall back to defaults: never fatal.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if self.ephemeral {
            return;
        }
        let Some(path) = Self::path() else { return };
        if let (Some(dir), Ok(text)) = (path.parent(), toml::to_string(self)) {
            let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(path, text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_junk() {
        let s = Settings {
            username: Some("magnus".into()),
            piece_set: Some("fantasy".into()),
            board: None,
            sidebar_hidden: true,
            dark: true,
            muted: false,
            ephemeral: false,
        };
        assert_eq!(
            toml::from_str::<Settings>(&toml::to_string(&s).unwrap()).unwrap(),
            s
        );
        let partial: Settings = toml::from_str("board = \"wood4\"\nfuture_key = 1").unwrap();
        assert_eq!(partial.board.as_deref(), Some("wood4"));
    }
}
