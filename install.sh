#!/bin/sh
# xclaude installer/updater: picks the best available method for your system,
# in order:
#   1. Homebrew (macOS/Linux)   2. .deb/.rpm (Debian/Ubuntu · Fedora/RHEL)
#   3. mise (github backend)    4. cargo (build from source)
#   5. prebuilt binary tarball from the GitHub release
#
# There is no prebuilt binary for Intel (x86_64) macOS (GitHub has no
# capacity-viable x86_64 mac runners; jobs queue for hours), so there
# `xclaude` is built from source (Homebrew's formula compiles too); every
# other target is prebuilt (Linux binaries are static musl builds).
#
# Re-running upgrades an existing install to the latest release (every branch
# installs-or-updates in place), so the same one-liner covers both:
#   curl -fsSL https://raw.githubusercontent.com/LLawli/XClaudeDashboard/main/install.sh | sh
#
# Everything is wrapped in main(), invoked on the last line: when piped from
# curl, sh's stdin IS this script, so a child that reads stdin would otherwise
# swallow the not-yet-parsed remainder of the file. The wrapper forces sh to
# consume the whole script before any command runs and doubles as a guard
# against executing a truncated download. main() runs with stdin from
# /dev/null so no child can ever block on (or eat) the pipe; sudo still
# prompts fine, it reads /dev/tty directly.
set -eu

REPO="LLawli/XClaudeDashboard"
TAP="LLawli/tap"

have() { command -v "$1" >/dev/null 2>&1; }
say()  { printf '==> %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

# Run a command as root: directly when already root, via sudo otherwise.
as_root()  { if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi; }
can_root() { [ "$(id -u)" = 0 ] || have sudo; }

latest_tag() {
  resp="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest")" || return 1
  # Isolate the tag_name field first: the API may return minified JSON on one
  # line, where a blind `cut -f4` would grab the first string value (`url`).
  printf '%s\n' "$resp" | grep -o '"tag_name"[^,]*' | head -1 | cut -d'"' -f4
}

# Resolve the latest release tag into $tag, or abort. Memoized: fall-through
# paths must not burn a second unauthenticated API request (60/hour/IP limit).
# (Called in the main shell, so `die` here exits the script, unlike a `die`
# inside `$(...)`.)
resolve_tag() {
  if [ -z "${tag:-}" ]; then
    tag="$(latest_tag)" || die "cannot reach the GitHub API"
    [ -n "$tag" ] || die "could not find the latest release tag"
  fi
}

# sha256 of a file, using whichever tool is present (empty output if neither).
sha_of() {
  if have sha256sum; then sha256sum "$1" | cut -d' ' -f1
  elif have shasum; then shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# Rust target triple for the prebuilt tarballs (empty = build from source).
triple() {
  case "$(uname -s)/$(uname -m)" in
    Linux/x86_64)                  echo x86_64-unknown-linux-musl ;;
    Linux/aarch64 | Linux/arm64)   echo aarch64-unknown-linux-musl ;;
    Darwin/arm64 | Darwin/aarch64) echo aarch64-apple-darwin ;;
    *) echo "" ;;
  esac
}

# Build and install from a pinned release tag. --locked honors the committed
# Cargo.lock; --tag pins the release rather than compiling the default branch.
cargo_from_source() { # $1: reason for the log line
  say "cargo install ($1)"
  resolve_tag
  exec cargo install --force --locked --git "https://github.com/$REPO" --tag "$tag" xclaude
}

main() {
  have curl || die "curl is required"

  # xclaude is a companion viewer: without XClaudeUsage feeding the SQLite log
  # it has nothing to show. Said up front because most install routes below
  # end with an exec (no post-install message is possible).
  say "reminder: xclaude requires XClaudeUsage: https://github.com/SrDarf/XClaudeUsage"

  # An old tarball install can shadow a package-managed one on PATH; warn so a
  # route switch between runs (say, Homebrew appearing on the system) doesn't
  # leave the stale binary silently winning.
  if [ -x "${HOME}/.local/bin/xclaude" ]; then
    say "note: ${HOME}/.local/bin/xclaude exists; if this run installs via a package manager, remove that file so it doesn't shadow the new version"
  fi

  os="$(uname -s)"
  arch="$(uname -m)"

  # 1. Homebrew (any OS that has it). On Intel macOS the formula compiles from
  # source; elsewhere it installs the prebuilt binary. Explicit `brew update`
  # first: with HOMEBREW_NO_AUTO_UPDATE=1 set, third-party taps never refresh
  # and `upgrade` would report a stale version as current. `upgrade` covers an
  # existing install; if it isn't installed yet, upgrade fails and we move on
  # to install. If the whole route fails (tap unreachable, formula missing),
  # fall through to the other methods instead of dead-ending.
  if have brew; then
    say "Homebrew"
    brew update --quiet >/dev/null || say "brew update failed; tap data may be stale"
    if brew upgrade "$TAP/xclaude" 2>/dev/null || brew install "$TAP/xclaude"; then
      exit 0
    fi
    say "Homebrew route failed; trying other methods"
  fi

  # Intel macOS: no prebuilt binary is published, so source is the only method.
  if [ "$os" = "Darwin" ] && { [ "$arch" = "x86_64" ] || [ "$arch" = "i386" ]; }; then
    have cargo && cargo_from_source "Intel macOS builds from source"
    die "Intel macOS has no prebuilt binary: install Rust (https://rustup.rs) and re-run, or use Homebrew"
  fi

  # 2. Linux distribution packages. Needs root (or sudo); when the arch has no
  # package or there is no way to become root, skip to the rootless routes
  # below instead of dying.
  if [ "$os" = "Linux" ] && [ -r /etc/os-release ]; then
    . /etc/os-release
    case "$arch" in
      x86_64)          deb_arch=amd64; rpm_arch=x86_64 ;;
      aarch64 | arm64) deb_arch=arm64; rpm_arch=aarch64 ;;
      *)               deb_arch=""; rpm_arch="" ;;
    esac
    case " ${ID:-} ${ID_LIKE:-} " in
      *" debian "* | *" ubuntu "*)
        if [ -n "$deb_arch" ] && can_root; then
          resolve_tag
          ver="${tag#v}"
          url="https://github.com/$REPO/releases/download/$tag/xclaude_${ver}_${deb_arch}.deb"
          say ".deb -> $url"
          tmp="$(mktemp)"
          trap 'rm -f "$tmp"' EXIT
          curl -fsSL "$url" -o "$tmp" || die "download failed: $url"
          as_root dpkg -i "$tmp"
          exit 0
        fi
        say "skipping .deb (no package for $arch, or no root/sudo); trying other methods" ;;
      *" fedora "* | *" rhel "* | *" centos "*)
        if [ -n "$rpm_arch" ] && can_root; then
          resolve_tag
          ver="${tag#v}"
          url="https://github.com/$REPO/releases/download/$tag/xclaude-${ver}-1.${rpm_arch}.rpm"
          say ".rpm -> $url"
          # -U installs or upgrades in place; --replacepkgs makes a
          # same-version re-run succeed instead of failing "already installed".
          if ! as_root rpm -U --replacepkgs "$url"; then
            if have dnf; then
              as_root dnf install -y "$url"
            else
              die "rpm install failed: $url"
            fi
          fi
          exit 0
        fi
        say "skipping .rpm (no package for $arch, or no root/sudo); trying other methods" ;;
    esac
  fi

  # 3. mise (prebuilt binary from the release via the github backend). Pin the
  # tag resolved from releases/latest: mise's own "latest" version-sorts tag
  # names, which would pick this repo's legacy 1.x releases over 0.x, and an
  # unpinned `mise use` keeps whatever version is already installed on re-run
  # instead of upgrading. Pinning fixes both: each run installs exactly the
  # current release.
  if have mise; then
    resolve_tag
    say "mise (github:$REPO@${tag#v})"
    exec mise use -g "github:$REPO@${tag#v}"
  fi

  # 4. cargo (build from source).
  have cargo && cargo_from_source "from source"

  # 5. Prebuilt binary tarball.
  t="$(triple)"
  [ -n "$t" ] || die "no prebuilt binary for $os/$arch: install cargo (or mise) and re-run"
  resolve_tag
  base="https://github.com/$REPO/releases/download/$tag"
  url="$base/xclaude-$tag-$t.tar.gz"
  bindir="${HOME}/.local/bin"
  mkdir -p "$bindir"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  say "binary -> $url into $bindir"
  curl -fsSL "$url" -o "$tmp/xclaude.tgz" || die "download failed: $url"
  # Verify the co-located checksum when a sha tool is available (guards a corrupt
  # or truncated download; not a substitute for a signature).
  if curl -fsSL "$url.sha256" -o "$tmp/xclaude.sha256" 2>/dev/null; then
    expected="$(cut -d' ' -f1 "$tmp/xclaude.sha256")"
    actual="$(sha_of "$tmp/xclaude.tgz")"
    if [ -n "$expected" ] && [ -n "$actual" ] && [ "$expected" != "$actual" ]; then
      die "checksum mismatch for xclaude-$tag-$t.tar.gz"
    fi
  fi
  tar -xzf "$tmp/xclaude.tgz" -C "$tmp"
  install "$tmp"/xclaude-*/xclaude "$bindir/xclaude"
  say "installed $bindir/xclaude; make sure $bindir is on your PATH"
}

main "$@" </dev/null
