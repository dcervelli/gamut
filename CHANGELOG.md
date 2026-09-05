# Changelog

Notable changes to `gamut`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Changed

- The grid toggle has moved from the end of the top bar to the head of the
  bottom one, in front of the button that says how a pixel is read out, and
  its spacing is now written after the mark rather than in front of it. The
  pixel button and the readout after it move along the bar as the toggle
  widens to read a spacing out.
- The pixel readout cycles on `.` rather than `>`; the Shift is gone. The two
  copies that take what it reads out are unchanged, still `Ctrl+.` for the
  value and `Ctrl+Shift+.` for the coordinate.
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
