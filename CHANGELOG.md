# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- One-line installer (`install.sh`): picks the best available method
  (Homebrew → `.deb`/`.rpm` → mise → cargo → prebuilt tarball) and
  installs-or-upgrades in place on re-run.

## [0.1.0] - 2026-07-07

### Added

- Initial release: real-time TUI dashboard (`xclaude`) for
  [XClaudeUsage](https://github.com/SrDarf/XClaudeUsage) token/usage data.
- Reads the XClaudeUsage SQLite log (`~/.claude/data/xclaude-usage.db` by
  default, overridable via `--db-path` / `XCLAUDE_DB`).
- Optional Turso cloud sync via `xclaude-cloud.json`
  (`--cloud-config` / `XCLAUDE_CLOUD_CONFIG`).
- Release pipeline: prebuilt binaries (Linux x86_64/aarch64 musl, macOS
  Apple Silicon, Windows x86_64) with sha256 checksums, `.deb`/`.rpm`
  packages, and Homebrew tap updates.

[Unreleased]: https://github.com/LLawli/XClaudeDashboard/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/LLawli/XClaudeDashboard/releases/tag/v0.1.0
