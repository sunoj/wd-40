# Scan-coverage investigation

KB consulted: `macos-du-double-counts-booted-simulators-in-coredevice-devicefs` (match: simulator
devices are the real consumer, removed through `simctl`, aged by newest file under `data/`, booted
devices left alone); `a-cleanup-that-reports-zero-reclaimable-only-measures-classes-it-knows`
(match: the gap is classes the scan does not know, not mis-measurement).

## Question

On a developer Mac with 417 GiB used of 460 GiB, why does a scan report only a small fraction of
the regenerable developer data, and which classes should discovery learn?

## Measurement

`du -xsh` over the home directory and the largest subtrees (logical sizes, APFS clones counted
once per path):

| Location | Size | Covered before |
|---|---:|---|
| `~/Library/Developer/CoreSimulator/Devices` (48 devices) | 109 GB | only each device's `data/Library/Caches` |
| `~/.aid/worktrees/<project>/<type>/<desc>/…` | 82 GB | no: the probe assumed `<repo>/<branch>/target` |
| `build/` beside a Makefile project, gitignored | 28 GB in one checkout, ~70 GB across worktrees | no: no recognized manifest |
| `~/Library/Developer/Xcode/{DerivedData,iOS DeviceSupport}` | 10 GB | yes |
| `~/Library/Caches/lima` | 1.6 GB | no |
| `~/Library/Caches/*.ShipIt` | 0.8 GB | no |
| `$TMPDIR` | 0.45 GB | no (small; left out) |

Within a simulator device the bytes are in `data/Library/Application Support`, `data/private` and
`data/var`; the `Caches` directory the scan already offered is about 5% of a device.

## Root causes

1. Simulator devices were treated as caches by sub-path. A device is state CoreSimulator owns, and
   it has to be reset (`simctl erase`) or removed (`simctl delete`, unavailable runtimes) through
   that tool, only while shut down.
2. A hard-coded depth for worktree containers. The ordinary walk already handles any depth; the
   container just was not one of its roots.
3. Build output was recognized only through a manifest list. A project's own `.gitignore` naming
   `build` or `dist` beside it is a stronger statement that the directory is generated output.

## Not covered, on purpose

- VM disk images (`~/.lima/<instance>`), agent session stores, task logs, and tool state
  directories: user data or another tool's state, not regenerable output.
- Xcode builds placed with a custom `-derivedDataPath`: no name to gate the walk on.
- `CoreDevice/DeviceFS`: a virtual mirror of booted devices; measuring it double-counts.

## Side finding: a waited-on scan starved

`wd40 scan` ran over 20 minutes with 0.7 s of CPU while the load average was ~770 on 12 cores,
7.9 of 9.2 GB swap was in use and under 3% of the disk was free. The sizing workers always ran at
background QoS, including for the CLI, where a person is waiting. Sizing now takes a
`qos::Priority`: the CLI, and a pass whose popover is open, run at utility; timer scans and
auto-clean stay at background.

The simulator work first added a `DeviceFS` check that called `canonicalize` on every directory
discovery and sizing read. It is now decided from path components alone, and sizing no longer asks
at all: no target root lies inside the mirror.
