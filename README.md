# Caturankam

`/sʌ.t̪ʊ.ɾʌŋ.ɡʌm/`

A super-lightweight native chess client for Lichess, written in Rust. No browser engine,
no Electron. Caturankam (சதுரங்கம்) is the Tamil name for chaturanga, chess's ancient
Indian ancestor.

> **Saying it:** four short syllables, *sa · thu · rang · gam*. The *c* is a soft *s*
> (Tamil ச), the *th* is a soft dental *t* (tongue touching the teeth, as in Tamil or
> Spanish), and the *r* is a single flick of the tongue.

Caturankam for Lichess is an independent project, not an official Lichess app.

<p align="center">
  <img src="docs/screenshots/light.png" width="49%" alt="Caturankam in light mode: a stone-coloured board mid-game, with a sidebar showing the moves and a Log in with Lichess button">
  <img src="docs/screenshots/dark.png" width="49%" alt="Caturankam in dark mode: the same position with the dark palette">
</p>

## What it does

Early draft, **Original** mode, macOS first.

- **Log in with Lichess:** OAuth with PKCE in your browser. The token stays in the macOS
  Keychain, never on disk.
- **Play online:** Rapid and Classical quick pairing, or the Lichess computer at levels
  1–8 (Blitz too). Clocks, draw offers, resign and abort. It reconnects if the connection
  drops.
- **Every Lichess piece set and board** (41 and 25), downloaded only when you pick one.
- **A Mac app:** native menu bar, light and dark appearance, a collapsible sidebar, and a
  shortcut for every action.
- **A local board** to play both sides offline.

## Keyboard

Every action has a single key, shown on its button. Menu items also have ⌘ shortcuts.

| Where | Keys |
|---|---|
| Anywhere | **F** flip · **S** sidebar · **T** light/dark · **R** random pieces and board · **L** log in · **N** new local game |
| Lobby | **1–5** quick pairing · **6–0** play the computer · **[ ]** computer level · **C** colour |
| In a game | **A** abort · **X** resign (press twice) · **O** offer draw · **Y** accept draw · **Esc** decline or cancel · **↵** back to lobby |
| Promotion | **Q R B N** |

With the sidebar hidden, the window title shows both players and their clocks.

## How light is it?

Caturankam vs Lichess in Google Chrome, idle on a board. Same machine, same minute. Chrome
used a fresh profile with no extensions. Memory is the sum of physical footprint across
all of each app's processes. CPU is averaged over 20 seconds.

| | Processes | Memory | CPU (idle) |
|---|---:|---:|---:|
| **Caturankam**, idle board | 1 | **66 MB** | **0.0%** |
| Chrome + lichess.org (home page) | 11 | 614 MB | 11.1% |
| Chrome + lichess.org/analysis (board) | 10 | 619 MB | 1.4% |
| *Only the Lichess tab* (home / analysis), if Chrome is already open | 4 / 3 | 166 / 121 MB | 6.8% / 0.1% |

Caturankam uses about **9× less memory** than Lichess in a fresh browser, and still about
2× less than the Lichess tab alone in a browser that's already open. With a Lichess board
image and piece set loaded and a larger window, it measured about 92 MB.

- **Machine:** MacBook Pro 13" (M1, 8 GB), macOS 26.5, Chrome 155, 14 October 2026.
- **Caturankam:** run in demo mode (`CATURANKAM_DEMO=1`, see below): logged out, nothing
  downloaded.
- **Release binary:** 14 MB.
- **Lazy loading:** the network thread, piece sets and boards load only when used.

## Run

```sh
cargo run --release
```

### Screenshots and benchmarks

Demo mode gives a clean state that never touches your settings or login:

```sh
CATURANKAM_DEMO=1 CATURANKAM_MOVES="e2e4 e7e5 g1f3" CATURANKAM_SHOT=out.png cargo run --release
```

`CATURANKAM_DARK=1` switches to dark. With `CATURANKAM_SHOT` the window captures itself
and quits.

## Layout

| Crate | Role |
|---|---|
| `crates/core` | Game model on top of `shakmaty`. No I/O, no UI. |
| `crates/theme` | Theme manifest (`themes/*/theme.toml`) schema and loading. |
| `crates/lichess` | Lichess API client: login, streams, seeks, moves. |
| `crates/app` | The `eframe` app: board widget, screens, menu bar, design system. |

## To do

**Play**
- [ ] Premoves
- [ ] Sounds (move, capture, check, game start/end), lazy-loaded
- [ ] Challenge a friend by username (Blitz is allowed in direct challenges)
- [ ] Accept incoming challenges
- [ ] Correspondence games
- [ ] Claim victory when the opponent leaves (currently links to lichess.org)
- [ ] Step back through the move list during and after a game
- [ ] Bring back 5+0 against the computer (dropped to fit keys 6–0)

**Modes** (the point of the project)
- [ ] Theme system: every visual and sound from a theme pack, loaded from disk
- [ ] Mode selector
- [ ] **Zen:** soft palette, slow eased animations, ambient sound
- [ ] **Chaos:** screen shake, combo streaks, particles, escalating music
- [ ] **Ancient:** chaturanga/shatranj art, piece names, war drums

**Mac app**
- [ ] `.app` bundle with an icon, installable to Applications
- [ ] Code signing, so the Keychain doesn't ask again after every update
- [ ] Appearance: follow the system's light/dark setting
- [ ] First release: `v0.1.0` tag and a CHANGELOG

**Later**
- [ ] Windows build with native menus
- [ ] Post-game analysis with a local Stockfish (never during live games)
- [ ] Offline shatranj against a local engine
- [ ] User-made theme packs

## Licence

MIT. See [LICENSE](LICENSE) and [ASSETS.md](ASSETS.md) for asset licences.
`crates/app/src/mac_crash_guard.rs` is adapted from
[spotifast](https://github.com/crmne/spotifast) (MIT); its notice is kept in the file.
