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

- A region of the picture can be selected and copied. `x`, or the button
  under the copy button, asks for one; the next drag on the picture draws
  it, in the image's own pixels, and it stays up with eight handles to pull.
  Dragging inside it moves it, dragging anywhere else pans as before. The
  arrows move it a pixel — or the handle the pointer is resting on — and
  `Ctrl` with an arrow grows it that way. While the pointer is on it, it
  writes its size at its middle and each edge's coordinate inside that
  edge's mark, dropping whichever of them a region drawn small has no room
  for. `Space` fits the region to the window
  instead of the picture, and `Ctrl+C` and the copy menu copy the region
  instead of the whole. `x` again, or `Esc`, takes it off, and so does
  stepping to another file.
- JPEG XL (`.jxl`) opens: both the lossy and the lossless halves of the
  format, in either the bare codestream or the container. The color space is
  read from the file's own statement of it, HDR included, or from an ICC
  profile where it says it that way; depth is kept as authored, up to 16-bit
  integer or floating point; grayscale stays single-channel; and the
  orientation is applied. An animation shows its first frame. CMYK files are
  refused rather than approximated.

## 0.1.0 - 2026-09-06

First release.
