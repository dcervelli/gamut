# Changelog

Notable changes to `gamut`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Fixed

- A window no longer opens larger than the screen on a scaled display. The
  opening size is worked out in logical pixels rather than physical ones —
  winit takes the size a window is created with as logical, so a 4K monitor at
  2x was given a window twice the size meant — and every monitor is consulted
  rather than the first one enumerated, the window opening at the largest size
  that fits on all of them. It now takes up to two thirds of a monitor rather
  than 85%.

### Changed

- The grid toggle has moved from the end of the top bar to the head of the
  bottom one, in front of the button that says how a pixel is read out, and
  its spacing is now written after the mark rather than in front of it. The
  pixel button and the readout after it move along the bar as the toggle
  widens to read a spacing out.
- The pixel readout cycles on `.` rather than `>`; the Shift is gone. The two
  copies that take what it reads out are unchanged, still `Ctrl+.` for the
  value and `Ctrl+Shift+.` for the coordinate.
- A window opens no smaller than one both floating panels fit in, where the
  monitor has the room for it. A small picture used to open a window its own
  size, which left the histogram and the information toggles dead in it before
  anything had been pressed.
- The histogram panel stays off in a window with no room for it, as the
  information panel already did. Where there is no room for what one of them
  opens, its toggle is drawn dead, refuses the press, and says why it is dead
  when the pointer rests on it.
- The Wayland `app_id` and X11 `WM_CLASS` are now `com.dcervelli.gamut`, the
  reverse-DNS form a desktop expects, and the desktop entry and the icon are
  filed under that name to match. A window rule matching the old `gamut` class
  needs the new name; the command, the package and the window title are
  unchanged.

### Added

- `--size <W> <H>` opens the window at the given size rather than at the
  image's own. The pair is the whole window, in logical pixels; a tiling
  compositor takes it as the floating size.

## 0.1.0

Not released.
