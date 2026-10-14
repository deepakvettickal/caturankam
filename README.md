# Caturankam

*Chess, the way it began.*

A super-lightweight native chess client for Lichess, written in Rust. No browser engine,
no Electron. Caturankam (சதுரங்கம்) is the Tamil name for chaturanga, chess's ancient
Indian ancestor.

Caturankam for Lichess is an independent project, not an official Lichess app.

## Status

Early draft: a local board in the **Original** mode. Play both sides with click or drag,
with legal-move enforcement, move animation, promotion, board flip and a move list. Lichess login and
online play come next; Zen, Chaos and Ancient modes after that.

## Run

```sh
cargo run --release
```

## Layout

| Crate | Role |
|---|---|
| `crates/core` | Game model on top of `shakmaty`. No I/O, no UI. |
| `crates/theme` | Theme manifest (`themes/*/theme.toml`) schema and loading. |
| `crates/lichess` | Lichess API client (not started). |
| `crates/app` | The `eframe` app: board widget, screens. |

## Licence

MIT. See [LICENSE](LICENSE) and [ASSETS.md](ASSETS.md) for asset licences.
