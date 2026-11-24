# Window and popover

WD-40 has two surfaces: the popover that hangs from the menu bar item, and a
resizable window. Both show the same four screens (scan, cleaning, done,
settings) from the same `AppState`, built by the same view builders and driven
by the same actions and tasks. `src/surface.rs` decides which one is filled.

## One live surface at a time

- While the window is visible it is the live surface. The popover is closed
  when the window opens, and a click on the menu bar item brings the window
  forward instead of opening the popover.
- While the window is not visible the popover is the live surface. Its content
  is kept built even when it is shut, so opening it does not wait on a build.
- "Which surface is live" is read off `NSWindow.isVisible` and
  `NSPopover.isShown` each time. It is never stored: a transient popover
  dismisses itself without calling back.

A single live surface means there is one set of live view handles
(`src/live.rs`), so a size arriving or a target coming off patches exactly one
view tree.

## Opening and closing the window

- Open: `⌘O` or the "Window" button in the popover's scan footer, or "Open
  WD-40 Window" in the menu shown by a right click (or Control-click) on the
  menu bar item. There is no system-wide hotkey.
- Close: the close button or `⌘W`. Closing hides the window (`orderOut`); the
  `NSWindow` is created once, has `releasedWhenClosed` off, and is reused.
- The window cannot be minimised. A minimised window reports itself as not
  visible, which would make the popover the live surface beside it.
- Closing stops nothing. A scan or a clean in flight carries on, and the
  popover shows it from the same state the next time it is opened.
- Opening either surface stops an unattended auto-clean pass before it starts
  another removal, through the same hook.

## Activation policy and the Dock

The app stays `NSApplicationActivationPolicyAccessory` at all times. The window
has no Dock icon and does not appear in the app switcher; the menu bar item is
the way back to it. The app is activated when the window opens so it comes to
the front. Quitting is `⌘Q` on the settings screen or "Quit WD-40" in the
status menu; closing the window does not quit.

Because an accessory app has no main menu, `⌘W` is the key equivalent of a
zero-size button in the window, and the other shortcuts (`⌘R`, `⌘,`, `⌘O`) are
key equivalents of the buttons that show them.

## Layout

The popover is 380pt wide and each screen is one column. Builders read the
pane width from `src/layout.rs` rather than a constant.

In the window, scan and cleaning keep the popover's column on the left at full
height and give the rest to the usage map: a treemap of every target, sized by
its bytes and coloured by its group.

- Scan: the column is the disk gauge, the verdict line, the whole list and
  the clean button (see "The scan screen" below). On the map a ticked target
  is solid and an unticked one faint; a click on a tile ticks it, exactly as
  its row does, and the line under the map names the tile under the pointer.
- Cleaning: the column is the progress figures and a line per target with its
  status. The plate is the scan map itself, in the same frame with the same
  tiles (`layout::map_frame`, `treemap::cells`), so the picture does not jump
  when a clean starts. Targets outside the job stay faint and never move.
- Done: the column is the result (free space gained, the disk before and
  after, what was removed per group); the map shows how the run ended, with
  the cleared footprints scribed in the steel and untouched tiles in place.
- Settings: one surface edge to edge, with the settings in a 560pt reading
  column down the middle. No screen is a popover-sized card in the window.

## The scan screen

The screen answers one question, what can safely be cleaned, and says the
answer once.

- **Header** (`src/header.rs`): free space, the gauge and its legend. The
  middle segment is everything the scan found, labelled "Artifacts", or
  "Recent builds" in grey when none of it is old.
- **Verdict line** (`src/scan_verdict.rs`), under the legend: "N old · up to
  X can go" with the "Tick them" action beside it. N and X are
  `selection::old_targets`, the batch the action ticks and auto-clean takes,
  which leaves simulator devices and toolchains out. When only those are old
  the line says they are ticked by hand. Once the batch is ticked the line
  says so and the clean button states the size. The info mark's tooltip says
  what "old" means and that sizes are as scanned: builds sharing APFS clones
  overlap, so cleaning can free less than the sum. The line is a zone of its
  own, so a tick repaints it without rebuilding the gauge.
- **Groups** (`src/scan_rows.rs`, `src/menu_rows.rs`): one line each: tick
  box, name, count, total, and a disclosure mark. In the popover a folded
  group shows up to three of its old targets and its count reads "3 of 17";
  a click anywhere on the header line unfolds every target, old ones first,
  and another click folds it again. A group with nothing old is the header
  line alone, with a green check when some other group does have old
  targets. The window has the room and shows every group unfolded, with no
  mark. Which groups are unfolded is `AppState::open`; a rescan clears it.
- **Rows**: name, kind and age, size. Age is the one sign of old or recent.
  A cache row adds that removing it costs a download. The reveal-in-Finder
  control is on show only while the pointer is on the row, when the second
  line also becomes the directory's path.
- **Large groups** (`src/group_scroll.rs`): past seven rows a group scrolls
  inside itself and hands the wheel to the outer list at either end.
- **Footer** (`src/scan_footer.rs`): the clean button, armed only when
  something is ticked (otherwise a line of text in its place), the
  auto-clean receipt, and the shortcuts.

### All clear

When sizes are in and nothing listed is past the keep threshold
(`selection::nothing_old`), the verdict line reads "✓ All clear · nothing older
than 7d" with "next in Nd" on the right: the days until the first listed target
turns old (`scan_verdict::next_old`, the same per-row test, of every kind). The
info mark's tooltip names that target. The list below is the ordinary one,
every group a single folded line, so the screen still shows what is there and
how big it is. A full-screen "all clear" with the can at rest was tried and
dropped: it said less than the folded list.
The live patches in `src/live.rs` refuse a tree built for the other screen, so
the change is always a rebuild.

## The two figures on the done screen

The headline is the volume's own free space, read with `statfs` before and
after the run. A row is the size the scan measured for the directories that
were removed. They differ when a directory shares blocks with APFS clones of
it (see `apfs-clone-overcount.md`): removing one copy does not return shared
blocks. Free space also moves with whatever else is written during the run.
The rows are headed "size as scanned", and when the two differ by more than
5% a note under the rows names both figures.

## Cleaning animation and the done screen

The plate plays removals one at a time: the can crosses to a tile, then the
tile rises off the steel. A tile joins the queue only when its target's removal
has finished, so the plate can be behind the job and is never ahead of it. A
backlog plays faster (down to half length per lift) but no lift is skipped.
The done screen waits for the queue to drain (`live::lifts_left`); "Show the
result" skips the wait. With Reduce Motion on there is no queue and no wait:
each tile is simply gone when its target is.

Layout is frame-based, so the window rebuilds the current screen when it is
resized and when the system appearance changes.
