# Caturankam

A super-lightweight native chess client for Lichess, written in Rust. Mac first, Windows second.
Repo: https://github.com/deepakvettickal/caturankam (currently empty — start from scratch).

**Name:** Caturankam (சதுரங்கம்) — the Tamil name for chaturanga, chess's ancient Indian ancestor.
Tagline: *Chess, the way it began.*

## The idea

Lichess's engine, community and rules — with a choice of mood. Four modes, switchable from a
selector at the top of the window. The chess underneath is identical in every mode; only the
presentation changes.

| Mode | Feel |
|---|---|
| **Original** | Lichess as is. Clean, fast, familiar. The baseline. |
| **Zen** | Peaceful and calm. Soft palette, slow eased animations, ambient soundscape, no harsh sounds. |
| **Chaos** | Gamified, for dopamine addicts. Screen shake on captures, combo streaks, particle bursts, escalating soundtrack. |
| **Ancient** | Chess as in the 7th–10th century (chaturanga → shatranj). Elephant for bishop, counsellor/vizier for queen, chariot for rook. Carved pieces, cloth/stone board, war drums on captures. |

Each mode controls: **animations, pieces, the board, sounds/music.**

## Non-negotiable principles

1. **Lightweight.** No browser engine, no Electron, no webview. Target: sub-second cold start,
   low idle RAM, near-zero idle CPU. Treat regressions here as bugs.
   **Everything is lazy-loaded:** a feature costs no memory until it's used. The network thread
   starts only on login or a download; piece sets and boards load only when picked and are
   dropped on switch; audio, engines and screens follow the same rule.
2. **Themes are data, not code.** A theme is a bundle (manifest + assets). Game logic and the
   Lichess layer must never know which theme is active. Adding a mode = adding a theme pack.
3. **Fair play (Lichess ToS).** The Board API forbids engine assistance. Nothing in any mode may
   hint at moves, show evaluations, or highlight threats during a live game. Engine analysis
   (later milestone) is only for finished games and must be hard-gated from live play.
4. **Animations never cost clock time.** Moves are sent the instant the user releases a piece;
   animation is cosmetic and runs after. In fast time controls, effects (especially Chaos)
   must shorten or skip gracefully. Input is never blocked by an animation.
5. **Legibility beats aesthetics.** Every piece in every theme must be instantly recognisable in
   a 3+0 game. Ancient silhouettes stay distinct even when the art is historical.
6. **Server is the source of truth.** Game state and clocks come from the Lichess stream; the
   client interpolates clocks locally between events but always reconciles to the server.

## Stack

- **UI:** `eframe` / `egui`, board drawn with the egui `Painter` (custom widget). If Chaos needs
  shaders later, use an egui wgpu paint callback — don't switch to a game engine.
- **Chess rules:** `shakmaty` (legal moves, SAN/UCI, FEN).
- **Async / HTTP:** `tokio`, `reqwest` (streaming NDJSON from Lichess).
- **Lichess API:** evaluate `litchee` vs `lichess-api` vs `licheszter`; if none fits cleanly,
  write a thin client for just the endpoints we use (that's fine — the surface is small).
- **Auth:** OAuth2 Authorization Code + PKCE with a loopback redirect
  (`http://localhost:<port>/callback`). Lichess needs no app registration; `client_id` is any
  identifier. Scopes: start with `board:play` (+ `preference:read` if needed).
- **Token storage:** `keyring` (macOS Keychain). Never write tokens to disk in plain text.
- **Audio:** `rodio`.
- **Theme manifests:** TOML via `serde` + `toml`.
- **Errors/logging:** `thiserror` in libraries, `anyhow` in the app, `tracing` for logs.

## Workspace layout

```
caturankam/
├── Cargo.toml                # workspace
├── crates/
│   ├── core/                 # game model on top of shakmaty: position, move list, clocks,
│   │                         # premoves, game events. No I/O, no UI. Fully unit-tested.
│   ├── lichess/              # auth (PKCE), event stream, board game stream, make move,
│   │                         # seek/challenge. Emits typed events over channels.
│   ├── theme/                # theme manifest schema, loading, validation, asset resolution.
│   └── app/                  # eframe app: board widget, animation system, audio,
│                             # mode selector, screens. Depends on all of the above.
└── themes/
    ├── original/
    ├── zen/
    ├── chaos/
    └── ancient/
```

Communication: the `lichess` crate runs on a tokio runtime and sends events to the UI via
channels; the UI sends commands back. The egui thread never blocks on network I/O.

## Theme schema (starting point — refine as needed)

```toml
# themes/zen/theme.toml
name = "Zen"
id = "zen"

[board]
light = "#E8E4DA"
dark  = "#B9B2A3"
texture = "board.png"            # optional
coordinates = "subtle"           # none | subtle | full

[pieces]
set = "pieces/"                  # wK.svg, wQ.svg ... bP.svg
# Ancient may remap display names: bishop -> "Elephant", queen -> "Counsellor", rook -> "Chariot"
labels = { }

[animation]
move_ms = 260
easing = "ease_in_out_sine"
capture_effect = "fade"          # none | fade | particles | shake
shake_intensity = 0.0
particles = 0
fast_time_control_scale = 0.3    # multiplier applied in bullet/blitz (principle 4)

[audio]
move = "sounds/move.ogg"
capture = "sounds/capture.ogg"
check = "sounds/check.ogg"
game_start = "sounds/start.ogg"
game_end = "sounds/end.ogg"
music = "music/ambient.ogg"      # optional, loops
music_volume = 0.3
```

Chaos may add a `[chaos]` section (streak thresholds, combo multipliers) — the theme crate
should tolerate mode-specific extensions without the core caring.

## Assets & licensing

- Check the licence of every piece set, sound and track before committing it. Lichess's own
  piece sets carry mixed licences — do not copy them blindly.
- Until real art exists, use placeholder pieces drawn in code (simple shapes/letters) so
  development is never blocked on assets.
- Keep an `ASSETS.md` listing every asset, its source and licence.
- Branding: "Caturankam for Lichess" is fine; never present it as an official Lichess app.

## Milestones

- **M0 — Skeleton.** Cargo workspace, eframe window, empty board drawn from a FEN, CI
  (fmt, clippy, test) via GitHub Actions. MIT or Apache-2.0 licence, README.
- **M1 — Local board.** Click/drag to move, legal-move enforcement via shakmaty, move
  animation, promotion picker, flip board. Play both sides locally. No network.
- **M2 — Lichess login.** PKCE loopback flow, token in Keychain, show logged-in username.
- **M3 — Play online (Original).** Seek a game, stream the game, send moves, clocks,
  resign/draw/abort, game-over screen. Premoves. Reconnect on stream drop.
- **M4 — Theme system + Zen.** Extract everything visual/audible into the `theme` crate.
  Mode selector at the top. Ship Original + Zen. **This is v1 — ship it.**
- **M5 — Chaos.** Effects, streaks, escalating music, all respecting principles 3 & 4.
- **M6 — Ancient.** Historical art direction, drums, piece labels.
- **Later:** Windows build, post-game analysis with local Stockfish (hard-gated from live
  play), offline shatranj variant vs a local engine, user-made theme packs.

## Git identity

All commits use my **personal** GitHub identity (`deepakvettickal`), never a work identity.
Before the first commit, check `git config user.name` / `git config user.email` in this repo
and set them repo-locally (`git config --local ...`) if they're wrong. Ask me for the email if
unsure — don't guess.

## Tooling & plugins

Installed to help build this. Use them deliberately, not reflexively.

### ponytail — minimal code (primary style + review)
- Default stance: write only what the task needs; never cut validation, error handling,
  security or tests for logic with branches.
- `/ponytail-review` before each commit/PR; `/ponytail-audit` at the end of each milestone.
- Deferred shortcuts get a `shortcut:` comment; run `/ponytail-debt` at milestone end and
  log anything worth fixing as a GitHub issue.
- ponytail's "write less" does **not** override principle 1–6 above or the tests rule below.

### agent-skills (Addy Osmani) — engineering workflow
Use selectively. Relevant here:
- `/spec`, `/plan` at the start of each milestone (keep specs short — this file is the
  top-level spec).
- `incremental-implementation`, `test-driven-development` (for `core` and `lichess` crates),
  `api-and-interface-design` (crate boundaries, theme schema), `debugging-and-error-recovery`,
  `git-workflow-and-versioning`, `ci-cd-and-automation`.
- **Ignore** the web-specific parts (browser DevTools testing, Core Web Vitals, `/webperf`) —
  this is a native app.
- Review: use `/ponytail-review` as the single primary reviewer to avoid two conflicting
  review passes. Use agent-skills' `security-and-hardening` only for the OAuth/token work (M2).

### graphify — codebase knowledge graph (token saver)
- Not useful until there's real code. Start using it from **M3** onward.
- Run with `--code-only` (Rust is parsed locally via tree-sitter; nothing leaves the machine).
- Install the git hook (`graphify hook install`) so the graph rebuilds on commit — a stale
  graph is worse than none.
- Add `graphify-out/` to `.gitignore`.
- For questions about this codebase, prefer `graphify query` / `path` / `explain` over
  reading many files.

### REA — reverse engineering
- **Do not use it against Lichess.** Lichess is fully open source, so read the source instead:
  - `lichess-org/lila` — server, API behaviour
  - `lichess-org/chessground` — board UX reference (drag, premove, animation feel)
  - `lichess-org/mobile` — how an official native client talks to the API
- **Licence warning:** these are copyleft (AGPL/GPL). Study behaviour and protocols; do not
  copy code into this repo unless we deliberately choose a compatible licence. When in doubt,
  ask me.
- Never point REA at chess.com or any other service's client.

### OmniRoute — model gateway
- Not used for this project's core work by default. If I enable it later:
  - only route to providers not flagged `tos: avoid`;
  - never send secrets, tokens or `.env` contents through it;
  - keep the strongest available model for architecture, `lichess` auth, and anything
    touching fair-play rules — cheap/free models only for boilerplate, docs, and tests.

## How to work with me

- I know data engineering and GCP well, but I'm **new to Rust** — when you make a
  non-obvious ownership, lifetime, async or trait-design choice, explain it in a sentence or two.
  Don't over-explain the basics.
- Keep it terse. Small, reviewable commits per step, each one compiling and passing
  `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test`.
- Prefer few, well-chosen dependencies. Justify any new crate in one line.
- Before building against a Lichess endpoint, check the current docs at https://lichess.org/api
  (including any Board API time-control or rate-limit restrictions) rather than assuming.

## First task

Start M0: create the Cargo workspace with the four crates above, an eframe app that opens a
window titled "Caturankam" and draws an 8×8 board from the starting FEN using placeholder
pieces, plus a GitHub Actions workflow for fmt/clippy/test. Then propose the plan for M1.
