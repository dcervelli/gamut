# Changelog

Notable changes to `gamut`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## 0.2.0 - 2026-09-11

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

- Animated GIF, PNG, WebP and JPEG XL files play. An animation starts
  playing as it opens, at the speed the file says and for as many loops as
  it says, and a bar of controls appears under the picture: a frame back,
  play or pause, a frame on, a readout of which frame is up and how far into
  the animation that is, and a timeline to drag along. `Enter` plays and
  pauses, `n` and `N` step a frame and stop there, and `--paused` opens the
  file stopped on its first frame. Everything that reads the picture — the
  pixel under the pointer, the histogram, a copy, the information panel —
  reads the frame on screen; the window, exposure and tone curve you set
  stay set from frame to frame. The frames are decoded ahead of the clock
  on a thread of their own, into a gigabyte at most: a file that fits is
  held whole, so stepping and scrubbing are instant, and a longer one is
  decoded again as the clock comes round to each frame. A file left part
  way through comes back where it was left.
- The other pictures in a file that holds several are reachable. A
  multi-page TIFF opens on its first page and an ICO on its largest icon, as
  before, and `n` and `N` — or the same bar, with the two step buttons and a
  count — walk through the rest.
- The file on screen can be handed to another program. A button under the
  copy button opens a menu of everything the desktop says can open a file of
  this kind — the same programs a file manager would offer, read from the
  desktop's own database and the associations you have set — with the default
  one first. Choosing one starts it with the file, and it goes on running
  after this window is closed. The button is there but dead, and says why,
  where nothing offers to open the file.
- A region of the picture can be selected and copied. `x`, or the button
  under the open button, asks for one; the next drag on the picture draws
  it, in the image's own pixels, and it stays up with eight handles to pull.
  Dragging inside it moves it, dragging anywhere else pans as before. The
  arrows move it a pixel — or the handle the pointer is resting on — and
  `Ctrl` with an arrow grows it that way. While the pointer is on it, it
  writes its size at its middle and each edge's coordinate inside that
  edge's mark, dropping whichever of them a region drawn small has no room
  for. `Space` fits the region to the window before the picture — the
  region fitted, then filling the window, then the picture's two fits, in
  turn — and `Ctrl+C` and the copy menu copy the region instead of the
  whole. `x` again, or `Esc`, takes it off, and so does stepping to another
  file.
- Holding `Space` while dragging a box on the picture zooms to the box when
  the drag lets go. `Space` now answers when it comes up rather than when it
  goes down, so that the picture does not move under the hand about to draw;
  a tap toggles the fit as before, and holding the key repeats nothing.
- The minimap can be used to go somewhere as well as to see where you are.
  Pressing on it centers the view on the point pressed, the moment the
  button goes down and as near as the image's edges allow, and holding on
  and moving keeps the view following the pointer.
- JPEG XL (`.jxl`) opens: both the lossy and the lossless halves of the
  format, in either the bare codestream or the container. The color space is
  read from the file's own statement of it, HDR included, or from an ICC
  profile where it says it that way; depth is kept as authored, up to 16-bit
  integer or floating point; grayscale stays single-channel; and the
  orientation is applied. CMYK files are refused rather than approximated.

## 0.1.0 - 2026-09-06

First release.
