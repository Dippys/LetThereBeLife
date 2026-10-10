# Development Guide

## Setup

- Rust stable ≥ 1.85 (edition 2024). The toolchain lives on Windows; from WSL use `cargo.exe`.
- VS Code: install the recommended Rust Analyzer + CodeLLDB extensions. **Run and Debug** has
  `Viewer (debug)`, `Viewer (release - smooth)`, and headless debug/release entries;
  **Run Task** has builds and `validate: workspace`.
- `cargo build` copies `config/simulation.toml` into `target/<profile>/config/`, so binaries
  can be launched directly from `target/`.

## Run

```sh
cargo run -p sim-viewer                                  # interactive (use --release for smooth)
cargo run -p sim-viewer -- --config path/to/file.toml
cargo run --release -p sim-viewer -- --valley           # the valley: 16 adults, 4 children, 24 deer, 2 wolves
cargo run -p sim-headless -- --ticks 600 --seed 42 [--agents 20] [--batch-size N]
cargo run --release -p sim-headless -- --canonical --agents 20 --batch-size 10000
```

`--canonical` runs the Phase 2 survival scenario: seed 1, a 2,048² world, 600,000 ticks, and
fixed water/wood spawn cohorts with starting supplies. It prints a versioned report and a hash.

### Configuration (`config/simulation.toml`)

| Key | Meaning |
|---|---|
| `simulation.seed` | World + simulation seed |
| `simulation.ticks_per_second` | Fixed tick rate (60) |
| `world.initial_width/height` | Bootstrap area loaded at start (4,096²). Not the world size (65,536²). |
| `world_cache.enabled/path` | Optional full-world archive (see below) |

### Full-world archive (optional, ~16 GiB, ~2 min)

```sh
cargo run --release -p sim-viewer -- --pregenerate-world
```

The archive lets the viewer show the whole world instantly and stream exact chunks while you
zoom. If it is missing, stale, or the seed doesn't match, the viewer warns and generates
procedurally instead. Re-run the command after changing the seed or the generator.

## Behavior study

Measures how viewer-like agents actually fare. They spawn without supplies and run headless:

```sh
cargo run --release -p sim-headless -- --study [--near-water | --groups | --valley | --apart] [--seed N]
    [--agents N] [--ticks N | --years N] [--mind legacy|memory|sharing|full] [--no-help] [--food PERCENT]
    [--no-wildlife] [--no-regrowth]
    [--verbose] [--trace AGENT]
    [--comms N] [--misreads N] [--lessons N] [--successes N] [--explain AGENT]
```

- `--near-water` spawns within 6 cells of fresh water. `--groups` drops agents in groups of 5.
  `--valley` is the spec's vertical slice: a 768² livable valley (found per seed) with two
  families camped at their own water, 8 adults and 2 children each (children start with no words
  and follow a parent); only the valley is simulated. `--valley --agents 16` is the adults-only
  band the success test uses. `--apart` puts each family of the band in its own valley of a
  neighboring pair (camps about 1,100 cells apart; about half of seeds have a pair). The default
  spawns on random land.
- The valley also releases 24 deer and 2 wolves (`--no-wildlife` to leave them out).
  `--food PERCENT` strips all but that share of the berries at the start, `--no-regrowth` stops
  picked bushes and trees from growing back (a famine valley), and `--no-help` turns off asking
  for food.
- The `food:` line counts meals by material, sickness, first tastes, and watched meals, and what
  founders and children ended up believing about bitter berries. The `wildlife:` line counts hunt
  and flee decisions, warnings and calls to hunt, strikes and kills (alone and together), bites,
  births, the animals left, and who fears wolves. The `animals:` line counts shouted calls and how
  many listeners acted on them. The `fire:` line counts hearths built, warm-ups, and who knows
  hearths warm. The `episode funnel:` line shows how far candidate success episodes
  got (consequence lesson → from a gesture → misread → acted on and about the misreading → speaker
  learned). The `requests for food:` line counts requests, how many
  were first misread, and the answers. The `children:` line shows how much of the founders'
  vocabulary the children picked up.
- The `communication:` line counts gestures by topic and receptions: informed, acted on,
  confirmed, abandoned, and misread. `--comms N` prints the first N exchanges that led
  somewhere, as stories: who pointed where, what they privately meant, how each watcher read it,
  and what each did and found. `--explain AGENT` prints that agent's personality, beliefs,
  acquaintances, and latest exchanges, followed by its decision trace.
- The `misreadings:` line counts receptions read differently from the sender's private intent,
  how many were acted on, and their recorded reasons. `--misreads N` prints the first N
  misunderstandings (acted on or not) as stories, with each listener's competing readings (for example
  `FOOD 50% / WATER 49%`) and why.
- The `repair:` line counts questions and repairs, corrections, word lessons by cause, and
  complete **SUCCESS EPISODES** (the project's definition of success). `--successes N` prints
  episodes step by step, and `--lessons N` lists lessons that changed a word's meaning.
- The `words:` line counts heard words in the first and second half of the run, and how often the
  listener already read the word the way the sender meant it. The `vocabulary:` line is the
  band's agreement on each place word at the start and at the end.
- `--years N` runs N years (one turn of the seasons, four simulated hours, 864,000 ticks each). Over decades the
  `families:` line counts couples within and across families, `births:` counts pregnancies,
  babies, children who started walking, and losses, `names:` counts names known (and how many
  differ from the given name) and names misheard, and `words passed down` shows, per generation,
  how many of their place words are still the founders' most common ones. `survivors` counts the
  living; `(collapsed N)` are alive but down.
- `--mind legacy` is the old reactive policy (what the viewer used before). `memory` adds the
  mental map. `sharing` adds gestures. `full` (the default) adds personalities and relationships.
- The `social:` line shows the % of time agents spend near another agent, their acquaintances,
  mean trust, and "I've been there" gestures. The `trait` lines split agents at each trait's
  midpoint and compare the behavior that trait should change. That's how you check that different
  personalities really behave differently.
- The output starts with a world summary: fresh water, food, how much land is near water, and the
  biome mix. Then come survivors, death causes, roaming, idle %, meals, and gestures.
- `--verbose` prints one line per agent, including what it remembers and its last need values.
  `--trace N` prints agent N's last 60 decisions (with failures) — the fastest way to see why an
  agent died.

Compare minds before and after any behavior change. Current numbers are in [STATUS.md](STATUS.md).

## Viewer controls

`H` (or the **? Help** button) shows the controls and a color legend; it opens on launch.

| Input | Action |
|---|---|
| Mouse wheel | Zoom around the cursor |
| Left-drag | Pan |
| Click a person | Open their panel: what they're doing and why, need bars, what they carry, what they believe (eats, avoids, hunts, fears, fire), their words, and friends. The map shows their remembered places (solid = seen, outline = hint) and dotted lines to people they know |
| `Tab` / `F` | Next living person / follow the picked person |
| `Esc` | Close help, then the build palette, then the person panel (it no longer quits) |
| `Space`, `1`–`9`, `+`/`-` | Pause, speed 1×–256×, step the speed (also buttons in the top bar) |
| `T` | Add a person at the cursor |
| `B` | Build palette: person, tree, berry bush, rock, water; click the map to place |
| `Shift+R` | Remove everyone (keeps terrain) |
| Right-drag, `C` | Select an area to generate (yellow = OK, red = too big); cancel pending generation |
| `F3` | Technical details: ticks, seed, chunks, the cell under the cursor, the picked person's raw values |

The top bar shows time, speed, and head counts. Hovering shows a one-line tooltip (person, animal,
bush with what's left, terrain). **Recent events** (bottom left) lists deaths, bites, kills, gifts,
warnings, collapses and recoveries, corrections, and misunderstandings ("Person 16 mistook
"kani" for water (meant berries)"); click one to look. Words appear for 2.5 s as bubbles over the speaker with the gesture
that went with them (uppercase when shouted), and a dotted line shows where the gesture pointed.

For screenshots: `--valley --advance TICKS --select PERSON --smoke-frames N --screenshot out.png`
renders hidden and saves the last frame.

## Tests

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
scripts/validate.sh [--quick] [--gpu]       # all of the above + checksum + headless smoke + success/slice tests
```

Where tests live:

- Unit tests live in a `tests.rs` or `tests/` folder next to the module they cover
  (e.g. `sim-core/src/engine/tests/`, `sim-world/src/worldgen/tests/`, `sim-viewer/src/render/tests/`).
- Public-API tests, one file per topic: `crates/sim-core/tests/` (`movement`, `routing_and_perception`,
  `needs`, `policy`, `resources`, `sleep`, `shelter`, `health`, `initial_supplies`, `spawned_objects`)
  and `crates/sim-world/tests/world_queries.rs`.
- `crates/sim-headless/tests/canonical_scenarios.rs`: canonical scenario determinism and soaks.
- `cargo test -p sim-world` runs only the world tests, which is handy when you're not touching the world.

Release harnesses and soaks are `#[ignore]`d. Run them explicitly:

```sh
cargo test --release -p sim-headless --test canonical_scenarios -- --ignored --nocapture --test-threads=1
```

Determinism fingerprints (last verified 2026-10-10, world generator v2). If one changes,
simulation behavior changed. Canonical runs use the legacy policy; the study covers the mind.

| Run | Hash |
|---|---|
| `sim-headless --ticks 600 --seed 42` (20 agents) | `561a5ca41553db95` |
| `sim-headless --canonical --agents 20` (600,000 ticks) | `358092051ec6b444` |
| `sim-headless --canonical --agents 100` (600,000 ticks) | `e4842c31783f1ea1` |

## Tooling

- **Map render** (BMP, no window):
  `cargo run --release -p sim-world --example render_map -- --width 4096 --height 4096 --step 8 --out map.bmp`
- **World-quality review set**: `cargo run --release -p sim-world --example render_map -- --review-set`
  writes 14 views × 4 seeds plus distribution/hash reports to `target/world-quality/`.
  Compare before and after any generator change.
- **Viewer metrics**: `SIM_VIEWER_SUMMARY_METRICS=1` logs summary-cache rebuild cost.
- **Drainage experiments**: `SIM_DRAINAGE_STEP`, `SIM_DRAINAGE_SEED`, and
  `SIM_RIVER_SOURCE_FLOW_THRESHOLD`. See [`archive/TESTING.md`](archive/TESTING.md).

## Deeper detail

`docs/archive/` holds the original long-form docs: per-slice plans, per-test coverage notes,
performance measurements, and all 58 architecture decisions. They are accurate as of 2026-07-17
but very dense. Use them as reference, and don't extend them.
