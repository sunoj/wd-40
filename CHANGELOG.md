# Changelog

## [0.6.5] — 2026-09-30

### Changed
- **Sizing reads each directory once** (`src/bulk.rs`, `src/sizes.rs`): the `getattrlistbulk` crate ignored the entry count the syscall returns and parsed the whole buffer, so a refill shorter than the one before replayed stale entries. Every directory was therefore read a second time with `readdir` to check it. The syscall is now called directly and exactly the returned records are parsed; a mount point, or anything inconsistent, falls back to the standard reader. Sizing the same 57 targets went from 3.0 s to 1.85 s (median of five), byte-for-byte identical. The crate is gone.

### Added
- **More of what a developer's Mac regenerates** (`src/rules.rs`, `src/roots.rs`, `src/cache_names.rs`): pip, uv, Go build, Gradle, Bun, CocoaPods, node-gyp, sccache, Deno and npx caches, and CoreSimulator's cache. In projects, each gated on its own marker: `.gradle`, `.turbo`, `.svelte-kit`, `.parcel-cache`, `.nuxt`, `.angular`, `.mypy_cache`/`.pytest_cache`/`.ruff_cache` (only with a valid `CACHEDIR.TAG`), `.tox`, `.nox`, Zig caches, `.dart_tool`, Foundry `out/`, Hardhat `artifacts/`, and a CMake `build/`. Virtual environments, Go's module cache, `~/.m2` and Playwright browsers are deliberately not offered.
- **Checkouts under `.claude/worktrees` and `.worktrees` are walked**, and a cargo target holding only cross-compiled output is recognised by the `CACHEDIR.TAG` and `.rustc_info.json` cargo writes at its root.
- **Default roots and depth** (`src/config.rs`): every existing one of `~/Develop`, `~/Developer`, `~/Projects`, `~/code`, `~/src`, `~/dev`, `~/workspace`, `~/repos`, `~/GitHub`, `~/Documents/GitHub`, at depth 8 (settings offers 4/6/8/10). A saved config keeps its own.

### Fixed
- **A directory called `build` or `dist` no longer hides what is under it** (`src/discover.rs`): the walk stops only at a directory it has validated as an artifact, including one in a group that is switched off. Findings from overlapping roots are listed once.
- **A Cargo manifest alone no longer makes `build/` or `dist/` an artifact** (`src/rules.rs`): Cargo writes neither, and crates keep build scripts in `build/`. Trunk's `dist/` is still found through `Trunk.toml`.

## [0.6.4] — 2026-08-14

### Changed
- **The exact reclaimable figure is measured when someone means to clean, not on a timer** (`src/tasks.rs`, `src/actions.rs`): working out what targets really occupy on the device costs 6.5 seconds across 217,851 files, of which `open` alone is 16 seconds of aggregate work. It cannot be cached — two investigations in `docs/` say why — and no metadata rule can safely skip a file that might share a block, because an APFS clone that was partly rewritten still shares the blocks nobody touched. So it runs on the first tick instead of after every scan, which takes it off the automatic ten-minute rescan entirely. The Clean button still starts removing the instant it is pressed.
- **A scan's total says "up to"** (`src/header.rs`): until that pass runs, the figure is a sum of allocated sizes, and on a disk full of `cp -Rc` clones that is a ceiling rather than an answer. The `≥` that means "still measuring" and the "up to" that means "settled, but a ceiling" never appear together.

### Fixed
- The legend row clipped "Free" off its right edge once the qualifier was added to it (`src/header.rs`). The qualifier now sits on the line below, where there is room, and the row has a test that fails if it overflows.

## [0.6.3] — 2026-08-14

### Fixed
- **The reclaimable figure could be stale, and was under-reporting by half** (`src/extent_cache.rs`, `src/reclaim.rs`, `src/cache.rs`): both physical caches decided a target's extents were unchanged from the target directory's *own* mtime and its total size. A deep write moves neither, so an in-place rewrite of a 64 MiB APFS clone left the cached union at 64 MiB while the disk held 128 MiB. They now also carry the newest timestamp *below* the target, which the sizing pass already computes — evidence that something happened, not proof that the extents held still, and the comments say so. Timestamps in the future are clamped, and the comparison keeps nanoseconds rather than whole seconds.
- **The scanning indicator sat on top of the text it was meant to sit beside** (`src/header.rs`): it was placed a whole `PAD_X` short of the right edge, over "N of M measured".

### Changed
- **The capacity gauge carries the scan** (`src/header.rs`, `src/motion.rs`): the reclaimable segment glides as measurements land instead of jumping, and while discovering — when there is no figure to show at all — a pale sweep travels the empty track. The spinner is gone. Under Reduce Motion there is neither, only the words.
- **Physical measurement runs on the worker pool the logical pass already uses** (`src/reclaim.rs`): 5.7 s to 4.5 s over 19 targets and 74,608 files. The comment claiming 71 seconds was measured against something else and is corrected.

## [0.6.2] — 2026-08-14

### Fixed
- **The scan no longer claims the disk is clean while it is still looking** (`src/scan_view.rs`, `src/tasks.rs`): during discovery there are no targets yet, which made `AppState::sizing()` false, which let the empty state draw. For the whole of that window the popover read "Nothing to clean — everything is tidy", and then filled with gigabytes.

### Added
- **The scan says what it is doing** (`src/header.rs`, `src/motion.rs`): "Finding build artifacts…" while it discovers, "Measuring build artifacts…" while it sizes, beside the count that was already there. One native spinner keeps moving through the stalls — sizing is I/O bound, and a single large target can go seconds without a figure to report. Under Reduce Motion there is no spinner at all, only the words.

## [0.6.1] — 2026-08-14

### Added
- **The rest of what Xcode leaves behind** (`src/roots.rs`, `src/cache_names.rs`): the SwiftPM package cache (`~/Library/Caches/org.swift.swiftpm`, 187 MB on the machine this was written on), `~/Library/Caches/com.apple.dt.Xcode`, and every `~/Library/Developer/Xcode/* DeviceSupport` — iOS, watchOS, tvOS and any sibling, found by enumeration rather than a hardcoded platform. DerivedData, ModuleCache and the per-simulator caches were already covered; these were not.
- **SwiftPM and Xcode project outputs** (`src/discover.rs`): `.build/` beside a `Package.swift`, and `build/` beside a `*.xcodeproj`. An `.xcodeproj` is a directory with a project-specific name, so it is matched by extension rather than looked up under a fixed one.

  A `dist/` beside either is deliberately **not** an artifact, and neither is a `build/` beside a `Package.swift`: SwiftPM writes to `.build/` and nothing else, and no Xcode tool writes to `dist/`. Whatever is in those directories was put there by hand. Archives are still never offered — an `.xcarchive` holds the only `.dSYM`s that make a shipped build's crash logs symbolicable, and it does not come back. Simulator devices are still never deleted: that is `xcrun simctl`, not `rm`.

### Changed
- **Releases are notarized** (`scripts/release.sh`, `scripts/verify-release.sh`): signing is no longer opt-in, the disk image carries its own signature and its own stapled ticket, and nothing is uploaded until the artifacts themselves — the app extracted from the zip, and the app inside the mounted image — pass a signature, ticket and Gatekeeper check. 0.6.0 went out ad-hoc signed because that check did not exist.
- **`artifact_types` is gone from `config.toml`**: a custom name in it was never collected, and its only working half — removing a built-in name — is what the per-group scan switches already do. Config files that still carry the key load unchanged.

## [0.6.0] — 2026-08-14

### Added
- **Rust toolchains** (`src/toolchains.rs`): a fifth scan group for `~/.rustup/toolchains`, which no scan root reached before — it lives under a dot-directory the walk refuses to enter, and its directory names match no artifact pattern. Six unused toolchains were sitting in 5.1 GB on the machine this was written on.

  A toolchain is not like the other four groups: it is global state rustup owns, it comes back as a network download rather than a rebuild, and it is removed with `rustup toolchain uninstall` — never by deleting the directory, which would leave rustup's record of what is installed lying. The default toolchain, anything `rustup override` points at, any channel a `rust-toolchain.toml` under the scan roots pins, and anything added with `rustup toolchain link` are never offered. When rustup's own settings cannot be read, nothing is offered rather than something guessed.
- **Cargo's crate downloads** (`src/roots.rs`): `~/.cargo/registry` joins the Caches group — tarballs, unpacked sources and the index, 1.3 GB on this machine, taken from `CARGO_HOME` rather than a hardcoded path. It goes in whole: dropping only the sources leaves the tarballs they came from, and dropping only the tarballs leaves sources cargo will not trust.
- **Disk panel** (`src/disk_panel.rs`): the menu opens on free space as a headline number, a capacity gauge splitting used / build artifacts / free, and what cleaning would leave. Free disk was a footnote in the header caption before.
- **Hover reveals the path** (`src/hover.rs`): a highlighted project row swaps its short name for the full path and the artifact's age. `NSMenuItem.toolTip` does not render in status-bar menus, so the row itself is the surface.

Both appearances were checked on a running app: the gauge track sits at 26% gray so it stays visible on a dark menu without competing with the filled part on a light one.

### Changed
- **The name column sizes itself** (`src/style.rs`): tab stops are measured from the names actually on screen instead of a fixed 28-character budget, so full names show wherever they fit; longer ones elide from the left and stay readable on hover.
- **Rows are shared fairly across groups** (`src/menu_rows.rs`): every non-empty group keeps up to 3 rows before the biggest group takes the rest, so a small group is no longer reduced to a header and "3 more not shown".
- **Same-named projects are disambiguated** (`src/names.rs`): two `web` projects read as `alpha/web` and `beta/web`.
- The app version moved from the header to a caption above `Quit`, leaving the top of the menu to the disk panel.

### Fixed
- `wd40` with no arguments panicked on `args[2..]` — the documented default invocation.

## [0.5.0] — 2026-07-29

### Added
- **Dedicated Settings window** (`src/settings_window.rs`, ⌘,): General (Launch at Login), Cleaning (auto-clean cadence, age threshold), Artifact Types to Scan, and Updates (auto-check + Check Now) in one panel. Artifact types had no UI before — they were config-file only. Replaces the Settings submenu so each setting has exactly one home; the informational Scan Rules submenu stays in the menu.

### Changed
- **Renamed to WD-40**: crate `rust-cleaner` → `wd40`, app bundle `Rust Cleaner.app` → `WD-40.app`, GUI binary `rust-cleaner` → `wd40-menu`, bundle id `com.wd40.rust-cleaner` → `com.wd40.app`. The `wd40` CLI and `~/.config/wd-40/config.toml` are unchanged.
- **Menu redesign**: rows are attributed strings laid out against shared tab stops, so name / size / usage-bar columns align; group headers carry SF Symbols and a secondary-colored count; the header shows total reclaimable space, app version, and free disk of total.
- **Settings submenu**: auto-clean cadence, age threshold, Launch at Login, automatic update checks, and per-group Scan Rules moved out of the main menu into `Settings ▸`, with native checkmarks instead of inline `✓` text.
- **`Rescan` is ⌘R and `Quit WD-40` is ⌘Q.**

### Added
- **Sparkle auto-update**: `Contents/Frameworks/Sparkle.framework` is loaded at runtime (`src/updater.rs`), so unbundled `cargo run` builds still start — they simply have no update menu item. Daily background checks plus an on-demand `Check for Updates…`.
- **`relay/`**: Cloudflare Worker + R2 that serves `appcast.xml` and signed build archives, authenticated by `UPLOAD_SECRET`.
- **`scripts/fetch-sparkle.sh`, `scripts/bundle.sh`, `scripts/release.sh`**: fetch + cache Sparkle, build a signed `dist/WD-40.app`, and publish an EdDSA-signed release to the feed.
- **Launch at Login toggle** (`src/autostart.rs`) that writes and bootstraps the `com.wd40.app` LaunchAgent from inside the app.

### Fixed
- **Launch at Login no longer fights the running app**: the toggle previously ran `launchctl bootstrap`, which started a *second* menu bar instance beside the running one, and `bootout`, which terminated the app mid-handler and left the plist behind so the toggle showed a stale "on". It now only writes or removes the LaunchAgent — launchd loads `~/Library/LaunchAgents` at login by itself.
- **Release notes can no longer break the update feed**: `]]>` inside `NOTES` closed the CDATA section early and produced a malformed `appcast.xml`, which would have broken updates for every installed copy. The terminator is now split across two sections and `scripts/release.sh` refuses to upload an appcast that fails `xmllint`.
- **`make install` removes the pre-rename install** (`Rust Cleaner.app` and its LaunchAgent), which would otherwise have left two menu bar apps starting at login.

### Removed
- `com.wd40.rust-cleaner.plist` and the `make autostart` / `make no-autostart` targets — the in-app toggle is now the single mechanism.


## [0.4.2] — 2026-04-18

### Fixed
- **Project name in menu bar**: targets under `~/.cargo-target/` now show their path relative to the shared root (e.g. `smart-router`, `smart-router/feat-xxx`) instead of `.cargo-target` or an indistinguishable `smart-router` repeated for every session

## [0.4.1] — 2026-04-18

### Changed
- **Per-session visibility under `~/.cargo-target/`**: list each `<project>/<session>/` session subdir as its own row so users can see the breakdown (previously only the project root was shown)
- **Nested size adjustment**: when a target is an ancestor of another target, the ancestor's reported size now subtracts the descendant's size — no double-counting, total matches actual disk usage

## [0.4.0] — 2026-04-18

### Added
- **Disk-aware size reporting** (`src/disk.rs`): query filesystem capacity via `statvfs`; sizes are clamped so a single target never exceeds the volume's total bytes
- **Shared `~/.cargo-target/` scan**: detects per-project cargo target roots under a shared `CARGO_TARGET_DIR`, with fallback to `<project>/<session>/debug|release` two-level layouts used by Claude Code session wrappers
- **Remaining disk space** surfaced via `AppState::remaining_disk_space`, using the first target's volume (or first scan dir) as reference

### Fixed
- **TOCTOU on delete**: `clean_all` / `clean_old` re-check the target is still a directory (not swapped to a symlink or removed) before `remove_dir_all`
- **Size overflow on clean**: freed-bytes accumulators now use `saturating_add` to avoid wrap on pathological inputs
- **Bulk-size overcount**: cap each directory's reported size at the enclosing volume's total bytes, falling back to `walkdir` when the bulk reader overshoots

### Changed
- `sum_bytes` helper extracted into `disk.rs` and shared between menu/CLI to keep accumulation consistent

## [0.3.0] — 2026-03-16

### Added
- **CLI binary (`wd40`)**: Terminal interface for scanning and cleaning dev artifacts
  - `wd40 scan` — display all artifact directories grouped by type
  - `wd40 clean` / `wd40 clean-old` — remove artifacts with optional `--dry-run`
  - Filter by group (`-g rust/node/build`), custom age threshold (`-d <days>`)
- **Auto-scan**: Menu bar app rescans every 5 minutes automatically
- **Two-phase scan**: Instant directory discovery, then background size computation
- **Shared library**: Core logic (`scanner`, `config`, `cleaner`) shared between GUI and CLI

### Fixed
- **getattrlistbulk infinite loop**: Parse errors in large directories caused 99% CPU hang; now falls back to walkdir on any parse error
- **Symlink following**: Bulk size API and discovery walker no longer follow symlinks (prevents cycles and inflated counts)
- **Size overflow**: Corrupt partial size data no longer reported; full fallback on error
- **SCANNING flag stuck**: Phase 2 thread wrapped in `catch_unwind` to guarantee UI reset

### Security
- Pre-delete symlink check: verify target is still a real directory before `remove_dir_all` (TOCTOU mitigation)
- `dir_size_fallback` no longer follows symlinks (`follow_links(false)`)
- `/tmp/cc-target-*` collection skips symlinks
- `--days 0` rejected to prevent accidental deletion of all targets

## [0.2.0] — 2026-03-15

### Added
- Grouped scan results: Rust, Node Modules, Build Output with per-group totals
- Multi-type artifact detection: `target/`, `node_modules/`, `.next/`, `dist/`, `build/`
- `/tmp/cc-target-*` temporary build directory detection
- Per-group clean and info panels in menu bar
- macOS `getattrlistbulk` API for fast directory sizing (~1,600x fewer syscalls)

## [0.1.0] — 2026-03-14

### Added
- Initial release: native macOS menu bar utility
- Scan `~/Develop` for Rust `target/` directories
- Visual status icon with "rust spots" indicating artifact size
- One-click cleaning of individual or all targets
