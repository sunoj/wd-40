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

In the window, scan and cleaning use a sidebar the width of the popover (disk
gauge, progress, the clean or stop button) and give the rest to the list or to
the cleaning plate. The scan list shows every row; the popover folds each group
after six. Done and settings are the popover's column, centred as a card.

Layout is frame-based, so the window rebuilds the current screen when it is
resized and when the system appearance changes.
