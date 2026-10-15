# Changelog

All notable changes to Caturankam. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Watch live:** follow the game on Lichess TV from the sidebar (**W**), with the
  players' titles, ratings and clocks. When it ends, the next one starts on its own.
  No login needed, and nothing connects until you press it.
### Changed

- **Installing is smoother.** Homebrew installs open without the macOS security prompt,
  and the README gives a one-line command for the DMG. Releases are signed with a stable
  certificate, so the Keychain no longer asks again after each update.
### Added

- **Material lead** in online games: `+4` beside the player who is ahead, as on Lichess.

### Changed

- The local game says **Move a piece to begin** before the first move, in smaller type.
- **Menu shortcuts match the sidebar's keys:** ⌘S sidebar, ⌘T light/dark, ⌘R randomize,
  ⌘F flip. **View ▸ Dark Mode** replaces the Appearance submenu.

### Removed

- **Game ▸ New Local Game** (the sidebar's **N** still starts one), and **View ▸ Show Tab
  Bar**, which macOS added on its own.
### Changed

- **A new app icon:** a rook drawn as a pencil construction sketch on graph paper, with a
  plain silhouette at the smallest sizes so it stays sharp in Finder and menus.

## [1.0.0] - 2026-10-14

The first release: a native Mac chess client for Lichess, in the **Original** mode.

### Added

- **Lichess login** with OAuth (PKCE) in your browser. The token is kept in the macOS
  Keychain, never on disk, and revoked on logout.
- **Online play:** Rapid and Classical quick pairing (rated or casual), and the Lichess
  computer at levels 1–8 including Blitz. Live clocks, draw offers, resign and abort,
  automatic reconnection, and resuming a running game.
- **Challenges:** challenge a friend by username (Blitz to Classical), and accept or
  decline challenges sent to you.
- **Premoves**, played the instant the opponent's move lands.
- **Claim victory** when your opponent has left the game.
- **Step through a game's moves** with the arrow keys or by clicking a move.
- **Sounds** for moves, captures, check, and game start and end, synthesised in code.
  **M** mutes.
- **Lichess's 41 piece sets and 25 boards**, downloaded only when picked and cached.
  Your Lichess choices are adopted on login. Randomize with **R**.
- **A Mac app:** native menu bar, light and dark appearance, a collapsible sidebar, and
  names and clocks in the window title when the sidebar is hidden.
- **A key for every action**, shown on its button.
- **A local board** for playing both sides offline.
- **Lightweight:** about 66 MB of memory and 0% CPU when idle; nothing loads until used.

### Fixed

- The app no longer quits unexpectedly when its window is closed on Macs with a
  Touch Bar (an AppKit bug).

[Unreleased]: https://github.com/deepakvettickal/caturankam/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/deepakvettickal/caturankam/releases/tag/v1.0.0
