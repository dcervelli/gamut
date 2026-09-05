# Changelog

Notable changes to `gamut`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

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
