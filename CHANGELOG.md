# Changelog

Notable changes to `gamut`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Changed

- The interface is drawn with [egui](https://github.com/emilk/egui). It keeps
  its shape — the four panels, the toggles in the strips, the two panels
  floating over the picture — and its controls are the toolkit's: the copy
  button opens a menu that prints each item's key beside it, the zoom readout
  and the pixel-format dot open popups, and the information column scrolls
  as any scroll area does. A menu that does not fit where it was hung is
  moved into the window rather than withheld. The interface is set in the
  desktop's own sans and monospace faces, as before.
- The oldest Rust that builds it is 1.95.

### Added

- JPEG XL (`.jxl`) opens: both the lossy and the lossless halves of the
  format, in either the bare codestream or the container. The color space is
  read from the file's own statement of it, HDR included, or from an ICC
  profile where it says it that way; depth is kept as authored, up to 16-bit
  integer or floating point; grayscale stays single-channel; and the
  orientation is applied. An animation shows its first frame. CMYK files are
  refused rather than approximated.

## 0.1.0 - 2026-09-06

First release.
