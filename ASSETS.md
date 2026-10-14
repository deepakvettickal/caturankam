# Assets

Every piece set, image, sound and track shipped with Caturankam, with its source and licence.

| Asset | Source | Licence |
|---|---|---|
| Original pieces (`themes/original/pieces/*.svg`) | Cburnett set, [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Chess_klt45.svg), unmodified | BSD-3-Clause (chosen from the author's GFDL/CC BY-SA/BSD/GPL options), see [`themes/original/pieces/LICENSE`](themes/original/pieces/LICENSE) |
| Original board and UI colours | Plain hex values in `themes/original/theme.toml` | MIT (this repo) |

Note: Lichess redistributes the Cburnett set under GPLv2+. Ours comes from the Wikimedia
originals, which offer BSD-3-Clause; don't swap in Lichess's copies.

## Downloaded at runtime, not shipped

The piece sets and boards in **Board & pieces** come from Lichess's CDN
(`lichess1.org/assets`) the first time a user picks one, and are cached in the user's cache
directory, just as lichess.org loads them in a browser. They are **not** in this repository or
in release builds, and each keeps its author's licence (listed in lila's
[COPYING.md](https://github.com/lichess-org/lila/blob/master/COPYING.md)). Don't commit them.
