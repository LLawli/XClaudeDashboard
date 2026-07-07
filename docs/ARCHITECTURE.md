# Architecture

`xclaude` is a companion **viewer** for
[XClaudeUsage](https://github.com/SrDarf/XClaudeUsage). XClaudeUsage's
statusline hooks record Claude Code usage into a SQLite database;
`xclaude` reads, aggregates and renders it. It never records usage itself,
and only writes to the database on the cloud-sync path (which mirrors
XClaudeUsage's own protocol). Keeping recording and viewing in separate
processes is the core design decision: the dashboard can crash, restart or
be absent without ever losing data.

## Data flow

```text
Claude Code sessions
        │  (statusline hooks from XClaudeUsage, a separate project)
        ▼
~/.claude/data/xclaude-usage.db          (SQLite, WAL)
        │  five_hour_window / seven_day_window   singleton window rows
        │  token_usage                           local per-token-type rows
        │  cloud_cache                           rows pulled from other devices
        │  cloud_outbox / cloud_state            sync queue + cursor/device id
        ▼
xclaude ──── polls PRAGMA data_version every tick (200 ms default)
   │
   ├── remote.rs ⇄ Turso/libSQL  (optional; push outbox, pull ≤500 rows/5h)
   └── pricing.rs ← LiteLLM model_prices JSON (claude-* only, 24 h disk cache)
```

## Module map

| Module | Responsibility |
|---|---|
| `main.rs` / `lib.rs` | Entry point, tokio runtime, color-eyre setup |
| `cli.rs` | Clap flags/env (`--db-path`, `--cloud-config`, `--tick-ms`) |
| `config.rs` | Default paths under `~/.claude/data/` |
| `app.rs` | Single async event loop: key/mouse/tick/sync messages → state |
| `event.rs` | Crossterm event stream → app messages |
| `tui.rs` | Terminal setup/teardown (alternate screen, raw mode, mouse) |
| `db.rs` | SQLite open (WAL, busy_timeout), `data_version` change detection |
| `window.rs` | 5h/7d window rows (schema differences aliased here) |
| `aggregate.rs` | Token aggregation by model (`token_usage`) and device (`cloud_cache`) |
| `rate.rs` | Per-minute burn-rate series and binning for the sparkline |
| `remote.rs` | Turso/libSQL sync: outbox push, pull, cache prune, device id |
| `pricing.rs` | LiteLLM fetch, `claude-*` filter, longest-prefix model matching |
| `ui.rs` | Fixed vertical layout, responsive gates, all cards |
| `widgets/` | Header hero card, stacked share bar, legend table |
| `style.rs` / `colors.rs` | Severity colors, heat ramp, 10-hue series palette |
| `format.rs` | Compact token counts (`1.2k`, `1.4M`), durations, money |

## Decisions and why

- **Poll `PRAGMA data_version` instead of watching the file.** The writer is
  a separate process using WAL; file-watching WAL databases is unreliable
  across platforms (writes land in `-wal`, not the main file). A 200 ms
  `data_version` poll on an already-open connection is cheap, portable and
  catches every committed write.
- **Read the DB, don't own it.** Schemas are defined by XClaudeUsage;
  `xclaude` treats them as an external contract (e.g. the 7d table's
  `starts_at` is aliased to `start_at` in `window.rs`, and `used_percentage`
  is range-guarded in `ui.rs` because the column is externally controlled).
- **Sync mirrors `syncCloud()`.** The Turso path replicates XClaudeUsage's
  JS implementation (same `token_delta` table, `INSERT OR IGNORE`, 500-row
  pull cap, 5 h retention) so both tools can sync interchangeably without
  coordinating.
- **Bundled everything.** `rusqlite/bundled` and `reqwest` with
  rustls + webpki-roots mean no system SQLite, OpenSSL or CA store, which
  is what makes fully static musl binaries (and the "runs on any distro"
  install story) possible.
- **Pricing is best-effort.** Costs come from LiteLLM's public JSON with
  longest-prefix model matching; missing prices render as `—` rather than
  blocking the dashboard. The cache (24 h TTL) keeps startup network-free.
- **Responsive by subtraction.** The layout is a fixed vertical stack;
  wide-only cards (usage gauge ≥130 cols, cost chart ≥110 cols in the
  breakdown band) drop out instead of squeezing. Rendering is fuzz-tested
  down to 1×1 without panicking.
- **Release pipeline: compile once, repackage many.** Each target is built
  once; tarballs, `.deb`/`.rpm` (nfpm) and the Homebrew formula all reuse
  the same binaries and checksums (see
  [`.github/workflows/release.yml`](../.github/workflows/release.yml)).
