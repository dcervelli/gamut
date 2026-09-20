# Changelog

Notable changes to `gamut`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Fixed

- The window opens at the picture's own size on a monitor at a fractional
  scale. Wayland gives a monitor's scale as a whole number until a window
  has a surface, so on one running 1.6 the size was worked out at 2, and a
  picture that should have opened at 100% opened at 80% — and the room the
  window was held inside was undercounted the same way, so a large picture
  opened smaller than it needed to. The true scale is now read from the
  compositor's `xdg_output` account of each monitor, on the connection that
  already reads its mode; where a compositor lacks the protocol the old
  reckoning stands.

### Changed

- `--timing` reports the upload on a line of its own, `upload <file>`,
  rather than inside the decode line's `gamut` share. The share took the
  upload in for every file but the first: the file named on the command
  line is decoded while the window is still being made and uploaded by
  the event loop once it exists, off the loader's clock. A large file so
  read as opening faster on its own than from a directory, when the only
  difference was where the upload was counted.

## 0.3.0 - 2026-09-20

### Added

- A help popup: `?` or `/`, or the button at the foot of the right strip,
  lists every key in the sections `--help` uses, with what each does and
  when it does anything. The same again, `Esc` or a click outside closes
  it.

- `w`, or the button beside the band under the histogram, marks the clipped
  pixels on the picture: red where every channel has gone to white, blue
  where every channel has gone to black. White is marked only where the
  surface is actually clipping it — no curve on, and no room above white —
  since a highlight rolled off or shown is not lost.

- The info panel reads a file's XMP as well as its EXIF: the title, the
  caption, the keywords, the creator, the rights and the program that
  wrote it, from JPEG, PNG, WebP, TIFF, HEIF and JPEG XL files. A file
  given a title by a cataloging program, which has an XMP packet and no
  EXIF block at all, used to show no metadata; it now shows the title.

- Camera raw files open: DNG, NEF, CR2, CR3, ARW, RAF, ORF, RW2, PEF, SRW
  and the rest of what LibRaw reads, developed through the system library
  with the camera's own white balance and matrix and nothing else — linear
  light, sixteen bits, in Rec. 2020, shown at 0–1 like the photograph it is.
  A raw under a `.tif` name is still recognized as one. The package now
  depends on `libraw`. A raw's thumbnail is the JPEG the camera wrote into
  it, turned the way the camera was held, so the chooser fills in
  milliseconds rather than at half a second a file. The information panel
  reads the metadata of every raw format — CR3, RAF, ORF, RW2 and MRW keep
  it somewhere a TIFF reader cannot see — and adds a Sensor section: the
  sensor and the picture inside it, the filter pattern, the white level,
  the white balance and the camera matrix. For a CRW, which has no
  metadata block at all, the header's own camera, exposure and time are
  shown instead.

- `Ctrl+P` opens a file chooser over the picture: type to filter the
  session's files by name — fuzzily, and by directory too when the list
  spans more than one, by the title the file's XMP gives it, or by place
  in the list with `:12` — arrow through the rows, and `Enter` or a click
  opens one. Each row shows a thumbnail, the name, the title where there
  is one, the kind of file, its place in the list and its size. Every
  file's header and title are read before any thumbnail is made, so the
  titles of a long list are all known within a moment of starting. The thumbnails are the desktop's own,
  read from and written to `~/.cache/thumbnails` so that a file manager
  and this program share them, and are made in the background at low
  priority from the moment the program starts.

- `--paste` opens on the image on the clipboard: it is written to the
  pictures directory and shown first, ahead of any path named, exactly as
  `Ctrl+V` would paste it once the window was up, so `gamut --paste` alone
  opens what was just copied. A clipboard with nothing to show is said on
  the terminal, and the paths are opened instead.

- `Ctrl+Shift` with an arrow shrinks a region that way a pixel, pulling
  its far side in — `Ctrl+Shift+Left` moves the right edge left — as
  `Ctrl` with an arrow grows it.
- JPEG-compressed TIFFs open. A scanned map or an aerial photograph as
  GDAL writes one stores its pixels as YCbCr, with the chroma at half
  resolution, and used to be refused as an unsupported color type; the
  pixels are now converted back to RGB, tile by tile across threads, with
  the coefficients and the coding range the file's own tags give, or the
  TIFF defaults where it gives none.
- A TIFF's transparency-mask directories are not pages. GDAL writes one
  after the picture and one after each reduced copy, and stepping to the
  next page of such a file used to fail on the mask, a one-bit image the
  decoder refuses; the pages are now the pictures alone, and the masks
  are stepped over.

### Changed

- Radiance and OpenEXR files open metered, the way a camera would expose
  the scene: the exposure is set to put the bulk of the light at middle
  gray, the neutral tone map rolls off what that leaves above white, and
  the histogram panel's slider shows the setting in stops. They used to open
  on the trimmed window, which on a scene with bright light sources — a
  sunlit window, a lamp — put white at the light and left the rest of the
  picture black; Debevec's memorial church opened with 97% of its pixels in
  the bottom code. A Radiance picture whose header states an `EXPOSURE=`
  other than 1 has already been scaled for viewing and opens as stored; the
  info panel shows the multiplier. Every other linear file — 16-bit and floating-point TIFF
  included — opens on the trimmed window as before, and the info panel's
  *Referred to* line now says which of the three a file is.

- The menu of copies and the menu of other applications are headed — `Copy`
  and `Open in…` — on the band the help popup's column headings sit on.

- `Ctrl+P` does nothing with a single file on the list, as the count it
  stands beside is not shown for one: there is nothing to choose.

- The keys a region takes are listed under a section of their own, `Region
  selection`, in `--help`, the manual page and the help popup. `Space` and
  the arrows each get a line there saying what they do with a region up,
  and their lines in `Zoom and position` say only what they do without
  one, rather than both in one sentence; `Ctrl+C` likewise gets a line for
  each, both under `Clipboard`.

- The window's rules are called the same thing everywhere: *stored*, *full*
  and *trimmed* — *manual* once a handle has been moved — on the histogram
  panel's buttons, in the bottom bar, in `e`'s help and on the command line
  as `--window stored|full|trimmed`, where the bar used to say `unit`,
  `min/max` and `99.8%` and the flag took `unit`, `minmax` and `pct`. The
  old spellings are still accepted.

- The histogram panel speaks in a viewer's terms rather than the display's.
  The band under the plot is a levels track: a handle at the value that
  comes out black and one at the value that comes out white, each dragged
  to where it should stand, and the band between them dragged to slide the
  window along the plot, as far as the plot goes. Either handle moves its
  own end of the window, on every file, and the exposure stays what it
  was: a push on top of whatever the window is. The `0.000–1.000` readout
  and the four nudge buttons beside it are gone.
  The keys are the handles' own: `a` and `s` step the black point, `A` and
  `S` the white point, each by a twentieth of the window's width along
  the plot and no further than the plot goes, where they used to slide
  the window and narrow or widen it about its center — a slide that could
  put black below the plot. The share
  of the picture the window is clipping is written in the two top corners
  of the plot — `0.5%` at black, `1.4%` at white — only when there is one,
  and only where the surface is actually clipping rather than showing or
  rolling off the highlights. The response curve is drawn only once the
  display is doing something, the line above the plot names the value
  under the pointer only while the pointer is over the plot, a graded
  file's axis no longer wears `0.0000` and `1.0000` at its ends, and the channel
  planes are drawn in a red, green and blue held short of the primaries.
  The rows under the band are the same for every file: *Exposure*, a
  slider over six stops each way with its reading at the end, in the
  quarter stops `d` and `f` count in; *Window*, its three rules named for
  what they do — *As stored*, *Full range*, *Trimmed* — where *As stored*
  is what puts a graded file's window back at 0–1 once a handle has moved
  it; and *Curve*, which `t` toggles.

- Dragging inside a region pans the picture, as dragging anywhere else
  does, so a region that fills the window no longer pins the picture under
  it. Moving the region is `Shift` with the drag, or a drag on the new
  handle at its center; its size is written under that handle rather than
  over it.

- The arrows move a region's current handle, drawn brighter than the
  others, rather than whichever handle the pointer happened to rest on.
  The center handle is current for a region just drawn, so the arrows move
  the whole of it as before; clicking or dragging another handle makes
  that one current, and the arrows move it a pixel at a time with the
  pointer anywhere. `Shift` with an arrow pans the picture a pixel under
  the region, as it does without one, rather than moving the region.

- The transport bar of an animation or a file of pages sits between the
  left and right strips, under the picture, rather than spanning the
  window as a second bottom bar.
- `Space` cycles through three views rather than toggling between two: the
  whole image, the window filled, then actual size. From a zoom chosen by
  hand it starts over at the whole image, as before. With a region up the
  picture's own three follow the region's two.
- The info panel's section of words about the file is headed *About*
  rather than *Description*, and the row that was *Description* is
  *Caption*: the heading no longer shares a word with a row under it.
- `--timing` prints to stderr rather than stdout, and its decode line
  names the file and splits the time into what the format's decoder took
  and what the program added around it — the header, the statistics scan,
  the metadata and the upload.
- The statistics scan every file gets on load — the range, the histogram
  and the plot — is divided by rows between rayon's threads. It was the
  larger part of opening a photograph, taking 50 ms on one thread for a
  file the decoder read in 10; it now takes a few.
- A TIFF's strips or tiles are decoded across threads, each thread reading
  its own rows of them from the file. A 14000×9600 LZW map that took 1.7 s
  to open takes 150 ms on a 32-core machine.
- Repacking a decoded image for the GPU — widening RGB to RGBA, and
  linearizing 16-bit and float samples — is divided between threads too.
  For the same map the widening took 260 ms on one thread and takes a
  fraction of that.
- An Ultra HDR JPEG's gain map is applied across threads, through a table
  rather than a power function per sample. A 12-megapixel phone
  photograph that took 560 ms to open takes 90, most of it the JPEG decode.
- A HEIC's tiles are decoded on as many threads as the machine has cores,
  where `libheif` would use four. An iPhone's 24-megapixel photograph
  opens in half the time on a 32-core machine.

### Removed

- The Reinhard tone curve. `t` now toggles between clipping the highlights
  and rolling them off with the neutral curve, the histogram panel's
  *Curve* row reads *Clip* and *Roll off*, the bottom bar says **rolled
  off** where it named the curve, and `--tone-map` takes `none` or
  `neutral` and refuses `reinhard`. Reinhard sends white to a half, so it
  re-graded the whole in-range picture to make room for the highlights;
  the neutral curve leaves everything below its shoulder where it was,
  which is the one thing a viewer wants of a curve.

### Fixed

- An idle window no longer redraws itself continuously, holding a core at
  whatever rate the surface allowed while nothing on screen moved. Two
  things asked for the next frame on every frame: the toolkit's answer to
  a redraw, which says "paint now" and was read as "paint again", and the
  interface's report of whether the pointer was on the picture, which
  counted as a change even when it had not changed.
- The info panel's column no longer wears a dark band under its header
  once scrolled, or above its foot while there is more below: the toolkit's
  fade at a scroll area's edge, painted in the panel's translucent fill,
  which came out as a shadow on a light theme.
- The histogram's row of tone curves is dead while a false color is on,
  and says why when rested on, and `t` does nothing there: a false color
  clips at the top of its ramp whatever the curve, which the picture, the
  bottom bar and the pointer's readout all knew, while the panel went on
  lighting a curve that was doing nothing and drawing it over the plot.
- A Radiance picture whose `#?RADIANCE` signature is not its first line
  opens. Radiance's own tools read a picture's header as text lines up to a
  blank one and know it by its `FORMAT=` line, so a `VIEW=` written in
  front of the signature — Debevec's `memorial.hdr` goes around this way —
  is a picture to them, and now to this program, which used to answer that
  the format could not be determined.

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
