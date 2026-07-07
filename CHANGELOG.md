# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-07-07

### Added

- One-line installer (`install.sh`): picks the best available method
  (Homebrew → `.deb`/`.rpm` → mise → cargo → prebuilt tarball) and
  installs-or-upgrades in place on re-run.

### Fixed

- Installer: the mise route now pins the tag resolved from
  `releases/latest` (mise's own "latest" version-sorted the legacy 1.x
  releases above 0.x and never upgraded on re-run), the Homebrew route
  refreshes the tap first (so `HOMEBREW_NO_AUTO_UPDATE=1` can't fake a
  successful "upgrade" to a stale version) and falls through to other
  methods on failure, the `.deb`/`.rpm` route works as root without sudo
  and skips to rootless methods instead of dying (unsupported arch, no
  root), same-version `rpm` re-runs succeed (`--replacepkgs`), and the
  GitHub API is only queried when actually needed (and at most once).
- Release pipeline: `.deb`/`.rpm` packages are built before the GitHub
  Release is created (published releases now carry every asset plus
  `sha256` checksums for the packages too), checksum computation for the
  Homebrew formula fails closed instead of shipping an empty `sha256`,
  pre-release tags (e.g. `v0.2.0-rc.1`) publish as prereleases and skip
  the tap, a `workflow_dispatch` pointed at a tag can no longer clobber
  released assets, nfpm is version-pinned, and the workflow token is
  read-only except for the release job.
- Pre-commit hook: checks the staged tree instead of the working tree,
  so unstaged edits can neither mask nor cause fmt failures.
- README: corrected the `.rpm` example filename (`xclaude-0.1.0-1.x86_64.rpm`)
  and documented the macOS checksum command (`shasum -a 256 -c`).

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

[Unreleased]: https://github.com/LLawli/XClaudeDashboard/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/LLawli/XClaudeDashboard/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/LLawli/XClaudeDashboard/releases/tag/v0.1.0
