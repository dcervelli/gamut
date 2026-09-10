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
  moved into the window rather than withheld.
- The interface is set in the faces the desktop itself is set in: the sans
  and monospace that fontconfig resolves, which is what `fc-match` prints
  and what the desktop's own rules name. Before, the faces were guessed from
  fontconfig's aliases alone, and could land on one the desktop never chose.
  Whatever the face, its capitals now sit at the middle of the bars and the
  buttons rather than wherever its own metrics happened to put them.
- The oldest Rust that builds it is 1.95.

### Added

- The file on screen can be handed to another program. A button under the
  copy button opens a menu of everything the desktop says can open a file of
  this kind — the same programs a file manager would offer, read from the
  desktop's own database and the associations you have set — with the default
  one first. Choosing one starts it with the file, and it goes on running
  after this window is closed. The button is there but dead, and says why,
  where nothing offers to open the file.
- JPEG XL (`.jxl`) opens: both the lossy and the lossless halves of the
  format, in either the bare codestream or the container. The color space is
  read from the file's own statement of it, HDR included, or from an ICC
  profile where it says it that way; depth is kept as authored, up to 16-bit
  integer or floating point; grayscale stays single-channel; and the
  orientation is applied. An animation shows its first frame. CMYK files are
  refused rather than approximated.

## 0.1.0 - 2026-09-06

First release.
