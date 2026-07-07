# XClaudeDashboard (`xclaude`)

Real-time TUI dashboard for your Claude Code usage: watch the 5-hour and
7-day rate-limit windows, burn rate, per-model / per-device breakdown and
estimated cost — live in your terminal, updating as you work.

Claude Code enforces usage windows, but you usually only notice you are
close to the cap when you hit it. `xclaude` answers, at a glance: *how much
of the window is left, how fast am I burning it, and will my budget survive
until the reset?*

> [!IMPORTANT]
> **`xclaude` requires [XClaudeUsage](https://github.com/SrDarf/XClaudeUsage)
> and has no standalone use.** It is a companion viewer: XClaudeUsage's
> statusline hooks record every Claude Code session into a SQLite log
> (`~/.claude/data/xclaude-usage.db`), and `xclaude` reads that log.
> Install XClaudeUsage first and let it run at least once — otherwise
> `xclaude` exits with an error because there is nothing to show.

```text
┌ 5h (h) ─────────────────────────┐┌ 7d (s) ─────────────────────────┐

┏ session 5h ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ started 09:04 • resets 14:04 • 02:31:07 left                      ┃
┃ output     312.4k / 500k   62%                                    ┃
┃ remaining  187.6k                                                 ┃
┃ ETA 100%   ✓ 3h 12m (rate 980/min · last 15min)                   ┃
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ burn rate ─────────────────────────────────────────────────────────┐
│ ▂▃▅▂▁▁▂▄▆█◆▅▃▂▁▁▂▃▂▁          now 980 • avg 640 • peak 2.1k/min   │
└────────────────────────────────────────────────────────────────────┘
┌ models ────────────────────────────────────────────────────────────┐
│ ███████████████████▓▓▓▓▓▓▓▓▓▓░░░░░░                                │
│ model               %     in    out   cache   read    cost         │
│ claude-fable-5     64%  1.2M   310k    2.1M   8.4M  $12.40         │
│ claude-haiku-4-5   36%  800k   120k    500k   1.2M   $1.80         │
└────────────────────────────────────────────────────────────────────┘
 5h  synced remote · pulled 12 · pushed 3    q quit  r refetch  [v] verbose
```

## Features

- **Two views**: the 5-hour session window and the 7-day window, switchable
  with one key (or a mouse click on the tabs).
- **Live**: detects new writes to the SQLite log within ~200 ms
  (`PRAGMA data_version` polling) — no restart, no manual refresh.
- **Burn rate sparkline** with per-minute binning, peak marker and an
  **ETA to 100%** that tells you whether the budget survives until the reset
  (`✓`) or runs out first (`⚠`).
- **Breakdown by model or by device** (`v` toggles): share bar plus a table
  of input/output/cache/read tokens and estimated cost per row.
- **Cost estimates** from [LiteLLM](https://github.com/BerriAI/litellm)'s
  public pricing table, fetched automatically and cached for 24 h.
- **Optional cloud sync**: if you use XClaudeUsage's Turso sync across
  devices, `r` pushes/pulls deltas and the device breakdown includes your
  other machines.

## Install

`xclaude` is a single self-contained binary. Pick whichever path is easiest —
every one ships the same binary.

### One-line install / update (picks the best method)

Runs anywhere and installs via the most appropriate route it finds — Homebrew,
your distro's `.deb`/`.rpm`, `mise`, `cargo`, or a prebuilt binary, in that
order. **Re-run it to update** — every route installs-or-upgrades in place:

```sh
curl -fsSL https://raw.githubusercontent.com/LLawli/XClaudeDashboard/main/install.sh | sh
```

Prefer a specific method? Pick one below.

### Homebrew (macOS / Linux)

```sh
brew install LLawli/tap/xclaude
```

### mise

[mise](https://mise.jdx.dev) fetches the prebuilt binary straight from the
release (via its `github` backend) and keeps it up to date:

```sh
mise use -g github:LLawli/XClaudeDashboard
```

### Prebuilt binaries (Linux, macOS, Windows)

Grab a tarball/zip from the
[releases page](https://github.com/LLawli/XClaudeDashboard/releases) —
Linux binaries are static (musl), so they run on any distro. Each release
lists `sha256` checksums; verify with:

```sh
sha256sum -c xclaude-v0.1.0-x86_64-unknown-linux-musl.tar.gz.sha256
# macOS has no sha256sum; use:  shasum -a 256 -c <file>.sha256
```

### deb / rpm

`.deb` and `.rpm` packages (x86_64 and aarch64) are attached to every
release:

```sh
sudo dpkg -i xclaude_0.1.0_amd64.deb   # Debian/Ubuntu
sudo rpm -i xclaude-0.1.0-1.x86_64.rpm # Fedora/RHEL (or dnf install ./…)
```

### Cargo

```sh
cargo install --git https://github.com/LLawli/XClaudeDashboard xclaude
```

Requires Rust 1.85+. Building from a clone works the same way:
`cargo build --release` produces `target/release/xclaude`.

## Usage

```sh
xclaude
```

That's it — it finds the XClaudeUsage database on its own. Options:

| Flag | Env var | Default | Description |
|---|---|---|---|
| `--db-path <PATH>` | `XCLAUDE_DB` | `~/.claude/data/xclaude-usage.db` | SQLite log written by XClaudeUsage |
| `--cloud-config <PATH>` | `XCLAUDE_CLOUD_CONFIG` | `~/.claude/data/xclaude-cloud.json` | Turso credentials for cloud sync |
| `--tick-ms <MS>` | — | `200` | DB change-poll interval |

### Keys

| Key | Action |
|---|---|
| `h` / `s` | Switch to the 5-hour / 7-day view |
| `v` | Toggle verbose mode (group by model ↔ by device) |
| `r` | Trigger a Turso cloud sync (push + pull) |
| `q` / `Esc` / `Ctrl+C` | Quit |

The tabs and the `[v] verbose` footer chip are also clickable.

### Terminal

Any UTF-8 terminal works; no Nerd Fonts needed. A truecolor (24-bit)
terminal is recommended for the full palette. The layout adapts to width —
the usage gauge and the cost chart appear on wider terminals and drop out
gracefully on narrow ones.

## How it works

`xclaude` is read-mostly: it aggregates the `token_usage` /
`cloud_cache` tables and the window rows XClaudeUsage maintains, and only
writes when syncing (mirroring XClaudeUsage's own `syncCloud()` protocol
against Turso/libSQL). Pricing comes from LiteLLM's
`model_prices_and_context_window.json`, filtered to `claude-*` models and
cached at `~/.claude/data/xclaude-prices.json`.

Architecture notes for contributors live in
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Releasing (maintainers)

```sh
git tag v0.1.0 && git push origin v0.1.0
```

The [release pipeline](.github/workflows/release.yml) builds each target
once, packages tarballs/zip with `sha256` checksums, attaches `.deb`/`.rpm`
built from the same binaries, publishes the GitHub Release with the
matching [CHANGELOG](CHANGELOG.md) section, and updates the
[Homebrew tap](https://github.com/LLawli/homebrew-tap) formula.

## Development

```sh
git config core.hooksPath .githooks   # once per clone: fmt check pre-commit
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

## License

[MIT](LICENSE)
