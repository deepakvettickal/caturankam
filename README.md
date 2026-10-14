# Caturankam

*Chess, the way it began.*

A super-lightweight native chess client for Lichess, written in Rust. No browser engine,
no Electron. Caturankam (சதுரங்கம்) is the Tamil name for chaturanga, chess's ancient
Indian ancestor.

Caturankam for Lichess is an independent project, not an official Lichess app.

## Status

Early draft, **Original** mode:

- Log in with your Lichess account (OAuth with PKCE; the token stays in the OS keychain).
- Play online: Rapid and Classical quick pairing, or the Lichess computer at levels 1–8
  (Blitz too). Clocks, draw offers, resign/abort, reconnect on dropped connections.
- All of Lichess's 2D piece sets and boards, downloaded on demand.
- A local board to play both sides offline.

Zen, Chaos and Ancient modes come later.

## Run

```sh
cargo run --release
```

## Layout

| Crate | Role |
|---|---|
| `crates/core` | Game model on top of `shakmaty`. No I/O, no UI. |
| `crates/theme` | Theme manifest (`themes/*/theme.toml`) schema and loading. |
| `crates/lichess` | Lichess API client: login, streams, seeks, moves. |
| `crates/app` | The `eframe` app: board widget, screens. |

## Licence

MIT. See [LICENSE](LICENSE) and [ASSETS.md](ASSETS.md) for asset licences.
