//! Lichess piece sets and boards, downloaded on first use and cached on disk.
//!
//! Nothing is bundled except the theme's own set: assets keep their authors'
//! licences and are fetched from lichess.org's CDN exactly as a browser would.
//! Only the selected set and board are held in memory; switching drops the old
//! ones, including egui's rasterised textures.

use caturankam_lichess::ASSETS;
use caturankam_theme::{PIECES, Theme};
use eframe::egui::{self, load::Bytes};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// 2D piece sets offered by Lichess (`modules/pref/src/main/PieceSet.scala`),
/// minus `monarchy`, which has no SVGs.
pub const PIECE_SETS: &[&str] = &[
    "cburnett",
    "merida",
    "alpha",
    "pirouetti",
    "chessnut",
    "chess7",
    "reillycraig",
    "companion",
    "riohacha",
    "kosal",
    "leipzig",
    "fantasy",
    "spatial",
    "celtic",
    "california",
    "caliente",
    "pixel",
    "firi",
    "rhosgfx",
    "maestro",
    "fresca",
    "cardinal",
    "gioco",
    "tatiana",
    "staunty",
    "cooke",
    "papercut",
    "minimal-warmth",
    "governor",
    "dubrovny",
    "shahi-ivory-brown",
    "icpieces",
    "mpchess",
    "kiwen-suwi",
    "totoy",
    "horsey",
    "anarcandy",
    "xkcd",
    "shapes",
    "letter",
    "disguised",
];

/// 2D boards offered by Lichess (`modules/pref/src/main/Theme.scala`): name, file.
pub const BOARDS: &[(&str, &str)] = &[
    ("brown", "brown.png"),
    ("wood", "wood.jpg"),
    ("wood2", "wood2.jpg"),
    ("wood3", "wood3.jpg"),
    ("wood4", "wood4.jpg"),
    ("maple", "maple.jpg"),
    ("maple2", "maple2.jpg"),
    ("horsey", "horsey.jpg"),
    ("leather", "leather.jpg"),
    ("blue", "blue.png"),
    ("blue2", "blue2.jpg"),
    ("blue3", "blue3.jpg"),
    ("canvas", "canvas2.jpg"),
    ("blue-marble", "blue-marble.jpg"),
    ("ic", "ic.png"),
    ("green", "green.png"),
    ("marble", "marble.jpg"),
    ("green-plastic", "green-plastic.png"),
    ("olive", "olive.jpg"),
    ("grey", "grey.jpg"),
    ("metal", "metal.jpg"),
    ("newspaper", "svg/newspaper.svg"),
    ("purple", "purple.png"),
    ("purple-diag", "purple-diag.png"),
    ("pink", "pink-pyramid.png"),
];

/// Twelve SVGs in `PIECES` order, with egui URIs unique to the set.
pub struct PieceSet {
    pub uris: [String; 12],
    pub svgs: [Bytes; 12],
}

impl PieceSet {
    pub fn builtin(theme: &Theme) -> Self {
        Self {
            uris: PIECES.map(|n| format!("bytes://{}/{n}.svg", theme.id)),
            svgs: theme.pieces.map(Bytes::Static),
        }
    }
}

/// A full-board image (Lichess boards are one picture, not per-square colours).
pub struct BoardImage {
    pub uri: String,
    pub bytes: Bytes,
}

/// What's currently on screen, plus downloads in flight. Download requests are
/// returned to the caller, which owns the network thread.
pub struct Assets {
    pub pieces: Option<(String, PieceSet)>,
    pub board: Option<(String, BoardImage)>,
    /// URL -> what it's for, while downloading.
    pending: HashMap<String, Pending>,
    /// Pieces of a set as they arrive; complete sets move to `pieces`.
    partial: HashMap<String, [Option<Arc<[u8]>>; 12]>,
}

enum Pending {
    Piece { set: String, index: usize },
    Board { name: String },
}

/// A download the network thread should run.
pub struct Fetch {
    pub url: String,
    pub cache: PathBuf,
}

impl Assets {
    pub fn new() -> Self {
        Self {
            pieces: None,
            board: None,
            pending: HashMap::new(),
            partial: HashMap::new(),
        }
    }

    /// Selects a piece set (`None` = built-in). Returns downloads still needed.
    pub fn select_pieces(&mut self, ctx: &egui::Context, set: Option<&str>) -> Vec<Fetch> {
        if self.pieces.as_ref().map(|(id, _)| id.as_str()) == set {
            return Vec::new();
        }
        if let Some((_, old)) = self.pieces.take() {
            for uri in &old.uris {
                ctx.forget_image(uri);
            }
        }
        self.partial.clear();
        self.pending
            .retain(|_, p| !matches!(p, Pending::Piece { .. }));
        // Names reach file paths and URLs, and settings.toml is user-editable:
        // only catalog names get through (no `../`).
        let Some(set) = set.filter(|s| PIECE_SETS.contains(s)) else {
            return Vec::new();
        };

        let mut fetches = Vec::new();
        let mut files: [Option<Arc<[u8]>>; 12] = Default::default();
        for (index, name) in PIECES.iter().enumerate() {
            let cache = cache_dir()
                .join("piece")
                .join(set)
                .join(format!("{name}.svg"));
            match std::fs::read(&cache) {
                Ok(bytes) => files[index] = Some(bytes.into()),
                Err(_) => {
                    let url = format!("{ASSETS}/piece/{set}/{name}.svg");
                    self.pending.insert(
                        url.clone(),
                        Pending::Piece {
                            set: set.into(),
                            index,
                        },
                    );
                    fetches.push(Fetch { url, cache });
                }
            }
        }
        self.partial.insert(set.into(), files);
        self.complete_set(set);
        fetches
    }

    /// Selects a board by Lichess name (`None` = theme colours).
    pub fn select_board(&mut self, ctx: &egui::Context, name: Option<&str>) -> Option<Fetch> {
        if self.board.as_ref().map(|(n, _)| n.as_str()) == name {
            return None;
        }
        if let Some((_, old)) = self.board.take() {
            ctx.forget_image(&old.uri);
        }
        self.pending
            .retain(|_, p| !matches!(p, Pending::Board { .. }));
        let name = name?;
        let file = BOARDS.iter().find(|(n, _)| *n == name)?.1;
        let cache = cache_dir().join("board").join(file);
        match std::fs::read(&cache) {
            Ok(bytes) => {
                self.board = Some((name.into(), board_image(file, bytes.into())));
                None
            }
            Err(_) => {
                let url = format!("{ASSETS}/images/board/{file}");
                self.pending
                    .insert(url.clone(), Pending::Board { name: name.into() });
                Some(Fetch { url, cache })
            }
        }
    }

    /// A finished download. Stale ones (the user switched since) are dropped.
    pub fn fetched(&mut self, url: &str, bytes: Vec<u8>) {
        match self.pending.remove(url) {
            Some(Pending::Piece { set, index }) => {
                if let Some(files) = self.partial.get_mut(&set) {
                    files[index] = Some(bytes.into());
                }
                self.complete_set(&set);
            }
            Some(Pending::Board { name }) => {
                if let Some((_, file)) = BOARDS.iter().find(|(n, _)| *n == name) {
                    self.board = Some((name, board_image(file, bytes.into())));
                }
            }
            None => {}
        }
    }

    pub fn failed(&mut self, url: &str) {
        self.pending.remove(url);
    }

    pub fn downloading(&self) -> bool {
        !self.pending.is_empty()
    }

    fn complete_set(&mut self, set: &str) {
        let Some(files) = self.partial.get(set) else {
            return;
        };
        if files.iter().all(Option::is_some) {
            let files = self.partial.remove(set).expect("checked above");
            let pieces = PieceSet {
                uris: PIECES.map(|n| format!("bytes://piece/{set}/{n}.svg")),
                svgs: files.map(|f| Bytes::Shared(f.expect("all present"))),
            };
            self.pieces = Some((set.into(), pieces));
        }
    }
}

fn board_image(file: &str, bytes: Arc<[u8]>) -> BoardImage {
    BoardImage {
        // The extension tells egui which decoder to use.
        uri: format!("bytes://board/{file}"),
        bytes: Bytes::Shared(bytes),
    }
}

fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("caturankam")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_unique() {
        let mut sets = PIECE_SETS.to_vec();
        sets.sort();
        sets.dedup();
        assert_eq!(sets.len(), PIECE_SETS.len());
        assert!(BOARDS.iter().any(|(n, _)| *n == "brown"));
    }

    #[test]
    fn unknown_names_never_reach_the_filesystem() {
        let ctx = egui::Context::default();
        let mut a = Assets::new();
        assert!(a.select_pieces(&ctx, Some("../../etc")).is_empty());
        assert!(a.select_board(&ctx, Some("../../etc")).is_none());
        assert!(a.pending.is_empty() && a.pieces.is_none() && a.board.is_none());
    }

    #[test]
    fn stale_downloads_are_ignored() {
        let mut a = Assets::new();
        a.pending.insert(
            "u".into(),
            Pending::Board {
                name: "wood".into(),
            },
        );
        a.pending.clear(); // as if the user picked another board
        a.fetched("u", vec![1, 2, 3]);
        assert!(a.board.is_none());
    }
}
