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
