//! Theme manifest schema and loading. A theme is data: nothing here knows
//! about game logic, and game logic never knows which theme is active.
//!
//! Unknown keys and sections (e.g. `[chaos]`) are ignored, so mode-specific
//! extensions don't break other themes.

use serde::{Deserialize, Deserializer};

/// Built-in default, compiled into the binary so the app always starts.
pub const ORIGINAL: &str = include_str!("../../../themes/original/theme.toml");

/// Piece file names (`<name>.svg` in the theme's piece set), in the order
/// [`Theme::pieces`] stores them: white then black, K Q R B N P.
pub const PIECES: [&str; 12] = [
    "wK", "wQ", "wR", "wB", "wN", "wP", "bK", "bQ", "bR", "bB", "bN", "bP",
];

macro_rules! original_pieces {
    ($($name:literal),*) => {
        [$(include_bytes!(concat!("../../../themes/original/pieces/", $name, ".svg")).as_slice()),*]
    };
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid theme manifest: {0}")]
    Manifest(#[from] toml::de::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Theme {
    pub name: String,
    pub id: String,
    pub board: Board,
    pub animation: Animation,
    pub ui: Ui,
    /// SVG bytes in [`PIECES`] order. Filled by the loader, not the manifest.
    // shortcut: built-in themes only; becomes owned bytes when themes load from disk.
    #[serde(skip)]
    pub pieces: [&'static [u8]; 12],
}

/// Window chrome around the board.
#[derive(Debug, Clone, Deserialize)]
pub struct Ui {
    pub background: Rgb,
    pub panel: Rgb,
    pub text: Rgb,
    pub accent: Rgb,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Board {
    pub light: Rgb,
    pub dark: Rgb,
    pub highlight: Rgb,
    #[serde(default)]
    pub coordinates: Coordinates,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Coordinates {
    None,
    #[default]
    Subtle,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Animation {
    pub move_ms: u32,
}

/// A `#RRGGBB` colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub [u8; 3]);

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        parse_hex(&s)
            .map(Rgb)
            .ok_or_else(|| serde::de::Error::custom(format!("expected #RRGGBB, got {s:?}")))
    }
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#').filter(|h| h.len() == 6)?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

impl Theme {
    pub fn parse(manifest: &str) -> Result<Self, Error> {
        Ok(toml::from_str(manifest)?)
    }

    pub fn original() -> Self {
        Theme {
            pieces: original_pieces!(
                "wK", "wQ", "wR", "wB", "wN", "wP", "bK", "bQ", "bR", "bB", "bN", "bP"
            ),
            ..Self::parse(ORIGINAL).expect("built-in Original theme is valid")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_parses() {
        let t = Theme::original();
        assert_eq!(t.id, "original");
        assert_eq!(t.board.light, Rgb([0xF0, 0xD9, 0xB5]));
        assert_eq!(t.board.coordinates, Coordinates::Subtle);
        assert!(
            t.pieces
                .iter()
                .all(|svg| svg.starts_with(b"<?xml") || svg.starts_with(b"<svg"))
        );
    }

    #[test]
    fn tolerates_extensions_rejects_bad_colours() {
        let ok = ORIGINAL.to_owned() + "\n[chaos]\nstreak = 3\n";
        assert!(Theme::parse(&ok).is_ok());
        for bad in ["#F0D9B", "F0D9B5", "#GGD9B5", "#F0D9B5FF", "#é0D9B"] {
            let m = ORIGINAL.replace("#F0D9B5", bad);
            assert!(Theme::parse(&m).is_err(), "{bad} should be rejected");
        }
    }
}
