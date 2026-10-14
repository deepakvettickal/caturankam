# Contributing to Caturankam

Thanks for wanting to help. Caturankam is small on purpose; a few rules keep it that way.

## Principles

1. **Lightweight.** No browser engine. Low idle memory, near-zero idle CPU. A
   regression here is a bug.
2. **Everything is lazy-loaded.** A feature costs nothing until it's used.
3. **Fair play.** Nothing may hint at moves, show evaluations or highlight threats in a
   live game ([Lichess fair play](https://lichess.org/page/fair-play)).
4. **Animations never cost clock time.** Moves are sent the moment a piece is dropped.
5. **Themes are data, not code.** Game logic never knows which theme is active.
6. **The server is the source of truth** for game state and clocks.

[CLAUDE.MD](CLAUDE.MD) has the full design notes.

## Getting started

You need macOS and the stable [Rust toolchain](https://rustup.rs).

```sh
cargo run --release
```

Before opening a pull request, make sure these pass. CI runs the same, plus a
packaging check:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

## Pull requests

- Branch from `main` and open a PR into `main`. PRs are **squash-merged**, so the PR
  title becomes the commit message: write it as one.
- Every PR needs passing CI and an approving review from the maintainer.
- Keep a PR to one change. Explain *why* in the description, and say how you tested it.
- Add a test for anything with logic in it.
- New dependencies need a one-line reason in the PR.
- Add user-visible changes to `CHANGELOG.md` under **Unreleased**.

## Assets

Every image, piece set, sound and font needs a licence that allows redistribution.
List each one in [ASSETS.md](ASSETS.md) with its source and licence. Lichess's piece
sets carry mixed licences, so check before committing anything.

## Releasing (maintainers)

1. Move the **Unreleased** entries in `CHANGELOG.md` under the new version, and bump
   `version` in `Cargo.toml`.
2. Add `packaging/release-notes/vX.Y.Z.md`.
3. Merge, then tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`. The Release
   workflow builds, tests and publishes. Run it by hand first for a dry run if
   packaging changed.
4. Update `Casks/caturankam.rb` with the new version and the SHA-256 from the
   release's `checksums.txt`.
