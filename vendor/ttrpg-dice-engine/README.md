# ttrpg-dice-engine

Dice notation parser, roller, and probability engine for TTRPGs.

```toml
[dependencies]
ttrpg-dice-engine = "0.1"
```

## Usage

```rust
use ttrpg_dice_engine::{roll, distribution, engine::LiveRng};

// roll 4d6 keep highest 3 (D&D ability scores)
let mut rng = LiveRng::new();
let result = roll("4d6kh3", &mut rng)?;
println!("Rolled {} (mean {:.1})", result.total, result.distribution_position.mean);

// theoretical distribution without rolling
let dist = distribution("2d6")?;
assert!((dist.mean - 7.0).abs() < 0.001);
```

## Notation

| Syntax | Meaning |
|---|---|
| `NdX` | Roll N X-sided dice |
| `dX` | Roll one X-sided die |
| `NdF` | FATE/Fudge dice (−1, 0, +1) |
| `d%` | Percentile die (d100) |
| `kh N` / `k N` | Keep highest N |
| `kl N` | Keep lowest N |
| `dh N` / `dl N` | Drop highest / lowest N |
| `!` | Explode on max |
| `!>=N` | Explode on result ≥ N |
| `!!` | Compounding explosion |
| `r N` / `ro N` | Reroll always / once |
| `mi N` / `ma N` | Minimum / maximum per die |
| `>N` `>=N` `<N` `<=N` | Count successes |

Expressions support `+`, `-`, `*`, unary `-`, and parentheses.

## Features

- **Exact probability** — full PMF via convolution for supported expressions
- **Distribution position** — every roll knows its percentile, mean, and std dev
- **System profiles** — named rolls and quirks for D&D, VtM, Call of Cthulhu, etc
- **Seeded RNG** — reproducible rolls for testing or replays
- **Serde** — the full AST and results serialize to JSON

## License

MIT
