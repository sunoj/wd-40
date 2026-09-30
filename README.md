# WD-40

> Dev artifact cleaner for macOS — menu bar app + CLI.

WD-40 finds and cleans validated build output and developer caches to reclaim disk space.

## Two Ways to Use

### Menu Bar App (`WD-40.app`)

A native macOS status bar utility with zero-config scanning.

- **Visual Status**: Icon gets "rustier" as build artifacts grow
- **Disk Panel**: Free space is the headline number, over a capacity gauge that shows the artifact slice in orange and what cleaning it would leave
- **Grouped Results**: Rust, Node Modules, Build Output, Caches, and Toolchains — every group keeps rows of its own, with aligned size columns and usage bars
- **Readable Names**: The name column sizes itself to the projects on screen, and same-named projects gain the directory that tells them apart
- **Hover for the Path**: Pointing at a row swaps the short name for its full path and how stale it is
- **One-Click Clean**: Individual projects, by group, all, or old only
- **Auto Scan**: Refreshes every 5 minutes
- **Auto Clean**: Configurable interval (1h/6h/12h/24h) + age threshold
- **Settings Window** (⌘,): Launch at Login, auto-clean cadence, age threshold, artifact types, and update preferences in one panel
- **Auto Update**: Sparkle checks the release feed daily; `Check for Updates…` runs it on demand
- **Two-Phase Scan**: Instant discovery, background size computation

### CLI (`wd40`)

Fast terminal interface for scripting and quick checks.

```
$ wd40
Scanning... found 17 targets

Rust — 2.7G
    764.3M    0d  [cc-target]  /tmp/cc-target-ai-dispatch-270
    338.0M    0d  [cc-target]  /tmp/cc-target-dev-cleaner-269

Node Modules — 1.6G
    351.0M    0d  [node_modules]  ~/Develop/ai/hiboss/node_modules
    308.9M    1d  [node_modules]  ~/Develop/ai/website-store/node_modules

Total: 4.3G in 17 targets
```

**Commands:**

| Command | Description |
|---------|-------------|
| `wd40` / `wd40 scan` | Scan and display all artifacts |
| `wd40 clean` | Remove all artifact directories |
| `wd40 clean-old` | Remove artifacts older than N days |
| `wd40 scan -g rust` | Filter by group: `rust`, `node`, `build`, `caches` |
| `wd40 clean-old -d 14` | Custom age threshold |
| `wd40 clean --dry-run` | Preview without deleting |

## Installation

### From Source

```bash
# Menu bar app → /Applications (downloads Sparkle, signs the bundle)
make install

# CLI → ~/.cargo/bin/wd40
make cli
```

Enable **Launch at Login** from the Settings window; it installs the
`com.wd40.app` LaunchAgent for the current user and takes effect at next login.

## Configuration

`~/.config/wd-40/config.toml` — shared by both app and CLI.

```toml
scan_dirs = ["/Users/username/Develop"]
max_age_days = 7
max_depth = 8
auto_clean_hours = 6   # 0 to disable
scan_groups = ["rust", "node_modules", "build_output", "caches", "toolchains"]
menu_bar_size = true
```

Without a saved config, WD-40 scans every existing `~/Develop`, `~/Developer`,
`~/Projects`, `~/projects`, `~/code`, `~/Code`, `~/src`, `~/dev`,
`~/workspace`, `~/repos`, `~/GitHub`, and `~/Documents/GitHub` (deduplicated
by canonical path, ignoring case). If none exists, it falls back to `~/Develop`.
Saved configs keep their existing roots and depth.

## Detection Rules

| Directory | Heuristic |
|-----------|-----------|
| `target/` | Contains `debug/` or `release/` |
| `node_modules/` | Parent `package.json` or package manager files/directories inside |
| `.next/` | Contains `cache/` or `static/` |
| `dist/`, `build/` | Project manifest; `build/` also accepts Xcode projects or `CMakeCache.txt` |
| `.build/`, `.gradle/` | Parent SwiftPM or Gradle manifest |
| `.turbo/`, `.svelte-kit/`, `.parcel-cache/`, `.nuxt/`, `.angular/` | Parent `package.json` or `angular.json` |
| `.mypy_cache/`, `.pytest_cache/`, `.ruff_cache/` | Valid `CACHEDIR.TAG` or tool-specific cache files |
| `.tox/`, `.nox/` | Parent Python test configuration |
| `zig-cache/`, `.zig-cache/`, `zig-out/`, `.dart_tool/` | Parent `build.zig` or `pubspec.yaml` |
| `out/`, `artifacts/` | Parent Foundry or Hardhat configuration |
| `/tmp/cc-target-*` | Auto-detected temporary Cargo build dirs |

The walk enters `.claude/` and `.worktrees/` to find nested checkout artifacts.
It leaves other hidden directories alone. Generic names like `build/`, `out/`,
and `artifacts/` are never selected or pruned without their marker.
Arbitrary `CACHEDIR.TAG` directories are not scanned, since checking every
directory would add a filesystem probe to the whole walk.

Fixed cache roots include Xcode and CoreSimulator, Cargo registry, Homebrew,
npm and npx, pnpm, Yarn, pip, uv, Go build, Gradle caches and wrapper downloads,
Bun, CocoaPods, node-gyp, sccache, and Deno. WD-40 does not offer Go module
caches, Maven repositories, Playwright browsers, virtual environments, or
`__pycache__` for deletion.

## Updates

The app ships with [Sparkle](https://sparkle-project.org) in
`Contents/Frameworks` and reads its feed from `SUFeedURL` in `Info.plist`.
Sparkle is loaded at runtime, so `cargo run` still works on an unbundled build —
the update menu item is simply absent there.

Releases are published to a Cloudflare Worker + R2 relay (see [`relay/`](relay)):

```bash
make release VERSION=0.5.0 NOTES="What changed"
```

`scripts/release.sh` builds the bundle, EdDSA-signs the zip with
`.sparkle/bin/sign_update`, writes `appcast.xml`, and uploads both.

Two secrets, both in the login Keychain. The signing key (service
`https://sparkle-project.org`, account `ed25519`) is **shared with sibling
apps** — never run `generate_keys -f`, it would overwrite the single global key
slot and break every feed. The relay's upload secret (service `wd40-release`,
account `UPLOAD_SECRET`) is read straight from the Keychain, so no release
command needs it on the command line:

```bash
security add-generic-password -U -s wd40-release -a UPLOAD_SECRET -w
```

Setting `UPLOAD_SECRET` in the environment still overrides the Keychain for a
one-off run. Whatever is stored here must match the Worker's own
`UPLOAD_SECRET`; rotating means writing both.

## Performance

- **Discovery**: Parallel directory walk with smart skip rules (hidden dirs, system dirs, symlinks)
- **Sizing**: macOS `getattrlistbulk` API — ~1,600x fewer syscalls than `stat` per file, with automatic fallback to `walkdir` on parse errors
- **Native**: Pure Rust + AppKit via `objc2` — no Electron, no web views

## License

[MIT](LICENSE)
