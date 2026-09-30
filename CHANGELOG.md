# Changelog

Notable changes to `gamut` as maintained overly verbosely by AI. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- **Edit configuration file**, a button at the foot of the help popup, and
  `Ctrl+,` on Linux: opens the configuration file in your editor, writing it
  first with every setting at its default where there is none, as the Mac's
  Settings item (`⌘,`) already did.

- The information panel's *Camera* section says more of how the picture was
  made: the exposure mode and whether it was bracketed, the metering, the
  white balance, the flash, a digital zoom beside the focal length, whether
  the camera merged several frames, and the owner and the camera's and
  lens's serial numbers. A lens by another maker than the camera's is named
  with its maker, and a lens the file does not name is given as its range.
- The information panel's *Image* section says what the file's orientation
  turns the picture by, and for a TIFF how its pixels are compressed.
- The information panel's *Location* section says how fast the camera was
  moving where the file records it, in the unit the file names: km/h, mph
  or knots.
- A map button after the coordinates at the head of the information
  panel's *Location* section opens where the picture was taken in the
  browser. Where it goes is the new `open_map_link` setting, geojson.io
  with a point at the coordinates unless it says otherwise, with `{lat}` and `{lng}` for the coordinates.

- The information panel has a *Regions* section for the regions a file's
  metadata marks out on the picture: the subject the camera found, and the
  regions its XMP marks, as cataloging programs do for the faces they find —
  in the Metadata Working Group's form, or as the people tags Windows Photo
  Gallery wrote, a face tagged both ways being shown once.
  It is headed by a dashed square, over a table of each region's subject
  — who is in it, or its kind where nothing says — and its top left corner
  and size, in pixels of the picture as shown, following the EXIF
  orientation and any turn, as the pointer's position reads. What else was
  written about it — its kind, a description, a barcode's value, whether a
  focus point was used — is in the subject's tooltip. A row copies as a
  line of CSV, and the heading as the table under its column heads.
  Resting the pointer on a row outlines its region on the picture, dashed,
  with its subject over it; resting it on the heading outlines every one.

- The information panel describes a picture's gain map under *Image*: whose
  description of it the file gives (ISO 21496-1 or Apple's), the map's size
  and whether it is one channel or three, how many stops above SDR white it
  lifts the picture, and how much of that lift the display is showing.
- The information panel has a *Depth map* section for a picture carrying
  one, after *Image*: the map's resolution and samples, and where the file
  says what its codes stand for, whose description it is (Apple's or
  Google's), whether the codes are spread over the distance or its inverse,
  the distances they run between, and whether those are measured or only
  estimated in scale — "relative", as an iPhone's dual-camera portrait says,
  with the range marked `≈` as the pixel readout marks it. A map whose file
  does not say what its codes stand for has its encoding given as unknown. The section describes the map whether the picture or the map is on
  screen, and while the map is, its heading wears a *Showing* pill.
- A **Depth** pixel format, beside Hex, Decimal and Mapped: for a picture
  carrying a depth map, the readout says how far away the pixel under the
  pointer was. An iPhone's portrait HEIC reads as a distance in meters,
  marked `≈` where the phone calls it an estimate, as it does for a
  dual-camera portrait; a JPEG with Google's depth block reads as a distance
  in the file's units; any other HEIC with a depth image reads as the code
  its map holds. A picture without one reads `(no depth)`.
- A **Depth** button at the bottom right, and `D`, for a picture carrying a
  depth map: shows the map in the picture's place, covering what the
  picture covered, and the picture again, without reading the file again.
  While it is up the map is the image, as a raw's camera JPEG is: the top
  bar gives its size, and the zoom, the pixel readout, the histogram, the
  display keys, a copy and an export all work on it.

### Changed

- On Linux, the configuration file opens in the editor `$VISUAL` or
  `$EDITOR` names, or else in the desktop's default for text files, in a
  terminal of its own where the editor's desktop entry asks for one. A
  default text editor that runs in a terminal, such as `nvim`, no longer
  starts with no terminal to run in.
- Keys, gestures and the `open_map_link` setting changed in the
  configuration file take effect as the file is saved, rather than at the
  next start.

- The information panel's *About* section comes straight after the file's
  own, before *Image*: what somebody wrote about the picture says what it
  is before its size and samples say how it is stored.
- The information panel's *Camera* section is split in two: *Camera* says
  what took the picture — the body, the lens, the owner and the serial
  numbers — and *Exposure*, under an aperture's mark, how this picture was
  taken. For a raw, *Exposure* gives the color temperature the white
  balance was set for, beside the white balance.
- The information panel's *Sensor* and *GPS metadata* sections are gone.
- A camera whose make is a company's whole name is no longer named twice:
  "NIKON D100" rather than "NIKON CORPORATION NIKON D100".
- The information panel no longer lists the *Image metadata* and *Capture
  metadata* sections, the rest of the file's EXIF field by field. What was
  worth reading in them is in *Camera*, *Exposure* and *Image* now.
- The information panel's *Georeference* section is headed by a map's mark
  over a table, as *Camera* is, and gives the ground a raster covers as
  *Min easting*, *Max easting*, *Min northing* and *Max northing* — or the
  same of longitude and latitude — rather than as two spans.
- The information panel's *About* section is headed by the file's title,
  beside the mark the panel's own button wears, or by *About* where there is
  no title. It holds only what somebody wrote — title, caption, comment,
  artist, keywords and copyright — and is left out where the file says none
  of them. The program that wrote the file and when it last did are listed
  with the rest of the file's metadata, since a camera fills both in on
  every file.
- The information panel opens on the file itself rather than on a column of
  fields: its name as the heading, with a file mark, then the folder it is
  in, then its size and how long ago it was modified — "366 bytes · 1 week
  ago" — each exact in its tooltip. A long name is cut in its middle,
  keeping its extension. The *Image* section is headed by the picture's size
  and its format — "4000 × 3000 JPEG" — with the rest of what it said as a
  table of two columns under it, which writes the layout in capitals:
  *8-bit RGB*. The *Camera* section is headed by the camera's name, with the
  rest in the same kind of table, the time it was taken first, said as how
  long ago with the date in its tooltip; the exposure
  is split into *Shutter speed*, *Aperture*, *ISO* and *Exposure
  compensation*, each copied on its own, where it was one line. The
  *Location* section is headed by the latitude and longitude beside a map
  pin, over the altitude, the direction the camera faced and how far out the
  fix may be. A click on any piece copies it, the size and the date
  exactly.
- Color spaces are called by their common names — *sRGB*, *Display P3*,
  *Rec. 2100 PQ*, *Adobe RGB (1998)* — in the top bar and the information
  panel, where they were written as primaries and curve, `BT.709/sRGB`. A
  pairing with no name is written as the two: *BT.2020, gamma 2.40*.
- The information panel's *Stored as* row, which named the GPU's texture
  format, is gone. In its place, a picture the GPU cannot hold at its full
  precision — 16-bit linear data on a GPU with no 16-bit integer textures,
  or float data on one that cannot filter 32-bit floats — has a *Precision*
  row saying so, and the first such picture in a run raises a warning.
- The pixel readout's formats — the value, and for a map the coordinates
  and how a latitude is written — are remembered between runs in the state
  file, as the sort order and the loupe's magnification are, rather than set
  in the configuration file. `pixel_format`, `coordinate_format` and
  `geographic_format` lines left in a configuration file are ignored.

### Fixed

- A BigTIFF, or a TIFF whose directory comes after its pixels, no longer
  loses a metadata field of more than 32 values: a GeoTIFF key directory of
  more than eight keys was one, and took the *Georeference* section with it.

## 0.8.3 - 2026-09-29

### Changed

- The top bar gives the file's name its room before the size, pixels and
  color space at its far end, which go one by one as the window narrows
  and are gone altogether before the name is cut short. Before, the facts
  kept half the bar whatever the name needed, and a narrow window showed
  the facts of a file it had no room to name.

### Fixed

- On macOS, associate images files with gamut at runtime.
- Remove homebrew script section that fails to associate files with gamut.

## 0.8.2 - 2026-09-29

### Fixed

- Update homebrew script to properly associate files with gamut.

## 0.8.1 - 2026-09-29

### Fixed

- Fix `cargo doc` build on Linux.

## 0.8.0 - 2026-09-29

### Added

- `gamut` builds and runs on macOS from source, with Homebrew's `libheif`
  and `libraw`. The file dialog, the clipboard, the Trash, the menu of other
  programs that open the file, the display's HDR room and the interface's
  faces are the Mac's own: the standard open dialog, the pasteboard, Finder's
  Trash, the applications Finder offers, the display's extended dynamic
  range, and San Francisco. The state file is kept in
  `~/Library/Application Support/gamut` and the thumbnails in
  `~/Library/Caches/com.dcervelli.gamut`; the configuration file stays in
  `~/.config/gamut`.
- On a Mac the keys held with `Ctrl` or `Alt` are held with `Cmd`, and the
  shortcuts every Mac program shares are there: `Cmd+0`, `Cmd+=` and `Cmd+-`
  zoom, `Cmd+Q` and `Cmd+W` quit, `Cmd+Backspace` moves the file to the
  Trash, `Cmd+[` and `Cmd+]` go back and forward. Two fingers on a trackpad
  pan, and a pinch or `Cmd` with the wheel zooms. Keys are spelled as a Mac
  spells them.
- A pinch on a trackpad is a gesture the configuration file can give a
  behavior, `gesture.image.pinch`, on either system.
- The configuration file reads `cmd`, `command`, `option` and `opt` as
  modifiers, beside `super` and `alt`, so one file serves both systems.
- On a Mac, a file opened in `gamut` from Finder, from "Open With", by a
  drop on its Dock icon or with `open -a` is shown, and joins the list of a
  window already open.
- On a Mac the interface wears the system's appearance: light or dark, with
  raised contrast where it is turned on, and the accent color chosen in
  System Settings. Changing either reaches a window that is already open.
- A Homebrew formula, which builds `gamut` from the release on the Mac it
  installs to and installs the manual page and the shell completions with
  it.
- On a Mac, `gamut` is an application, `Gamut.app`, with its icon in Finder,
  the Dock and Launchpad and its name at the head of the menu bar. Finder
  offers it under "Open With" for every format it reads. The Homebrew formula
  installs it, and `gamut` on the command line starts it.
- On a Mac, a menu bar of the program's own: File, Edit, View, Image, Go,
  Window and Help, holding what the buttons and keys do, each item with its
  key beside it and grayed out where it would do nothing. Settings (`Cmd+,`)
  opens the configuration file in the text editor, writing it first with
  every setting commented out if there is none; on Linux the same is the
  key name `interface.settings`, bound to nothing. `Cmd+L` and `Cmd+R` turn
  the picture, as in Preview.

### Changed

- A picture larger than the screen opens in a window up to three quarters of
  the screen's width and 85% of its height, where it was two thirds of each,
  so a portrait photograph on a laptop opens larger.

### Fixed

- `Ctrl` with `+`, `=`, `-` or `0` no longer scales the whole interface.

## 0.7.0 - 2026-09-28

### Added

- A file that is slow to open shows its thumbnail, enlarged to where the
  picture will be, while it loads, where the thumbnail has already been
  made; the thumbnails of the files either side of the one on screen are
  made ahead of the rest so that there is one. The histogram and information
  panels stop describing the picture being left and show a spinner in its
  place until the file is in. A file of a kind the last of
  which was slow is treated as slow from the key, so a folder of large files
  flips through thumbnails without a pause before each.
- A camera raw can be shown as the JPEG the camera wrote into it, with the
  camera's own curve and color, in place of the picture developed from the
  sensor. `v`, or the **Camera RAW** button that appears at the bottom right
  for a raw that carries a JPEG, switches between the two; the view stays
  where it was, so the two can be compared, and `Ctrl+E` exports whichever
  is up. The choice holds for every raw after it and is remembered between
  runs in the state file. A raw with no JPEG in it shows its developed
  picture and says so.
- `zoom.100.toggle` goes to actual size, and from actual size back to the
  whole image. It has no key by default.
- A configuration file with lines `gamut` cannot use is said in the window
  as it opens, as well as on the terminal: the first such line, and how many
  more there are. A key's line in the help now names a click that runs it
  as well as its keys.
- A single file opened by itself steps on through the other images in its
  folder, as a file manager's viewer does. The folder is read the first time
  `]`, `[`, `Ctrl+P` or `Tab` asks for more than the one file, and the step
  lands on the file that comes next in the file list's order; a slow read is
  said in the window with a count. `--alone`, or `browse_folder = false` in
  the configuration file, keeps the list to the file. `]` and `[` with nowhere
  to go now say so rather than doing nothing.
- The empty window left when the last file is deleted or taken off the list
  offers **Open all in** the folder that file came from, while that folder
  still holds images.
- A georeferenced raster can read out where the pixel under the pointer is
  on the ground: in the file's own projected coordinates and units, or as
  latitude and longitude on WGS 84, in decimal degrees or in degrees,
  minutes and seconds. The pixel readout's menu grows a **Coordinate** row
  and a **Latitude and longitude** row for such a file, and only for one;
  `,` cycles the coordinate and `<` switches the latitude's form. A file
  that cannot answer the choice — one with no named coordinate system, say
  — reads out the pixel, and the choice stands for the next map. The
  `coordinate_format` and `geographic_format` settings choose where they
  start, and `Ctrl+Shift+.` copies the coordinate as it is read out.

### Changed

- A step moves the count, the file list's highlight, and the name in the bar
  and the window's title at once, rather than when the file has loaded;
  they go back if it cannot be read. Until it has loaded, the copies,
  rename, remove, delete, export, **Open in**, the rotate buttons and the
  frame and page controls are unavailable, since the file named is not yet
  the one open. The size and format at the right of the top bar, the words
  such as **clipped** at the right of the bottom bar and the **Camera RAW**
  button go at once, and the frame and page bar becomes the new file's,
  rather than describing the previous file under the new name.
- A file, a folder or a sort taking its time is said after 150 ms rather
  than 250 ms, and a window opening on a list still being sorted waits that
  long for it before opening at the empty window's size.
- A raw's white is always the level the file states. A frame whose brightest
  pixel came within about half a stop of it used to be brightened to put that
  pixel at white, so the same scene opened brighter or darker depending on
  whether anything in it was near clipping.
- A double-click on the picture at actual size goes back to the whole image,
  where before it moved the detail under the pointer to the middle of the
  window; a double-click in and a double-click out is a quick look at the
  pixels. Its default is now `zoom.100.toggle`, and `1` still goes to actual
  size and nothing else.
- The desktop's file dialog starts in the folder of the picture on screen,
  or of the last one shown.
- A message in the window saying something failed begins with a capital,
  as the others do.
- The file list's sort and its direction are remembered between runs, in the
  state file beside the list's width and the loupe's magnification.
- The EPSG code a GeoTIFF names is now looked up, to find its latitude and
  longitude, where before it was only quoted. The table of codes it is looked
  up in is linked into the binary.
- A wide-gamut file — Display P3, Adobe RGB, BT.2020, a raw — is measured
  where the screen has it rather than in its own channels. A color sRGB
  cannot hold stands past white on the histogram, or below black, the
  corners count it, and `w` paints it; on an HDR surface it is shown and
  nothing is marked. The bottom bar's **clipped** says so for such a file,
  where before it was clipped in silence. A file in sRGB is measured
  exactly as before.
- The histogram of a 16-bit or float picture with a gain map is scanned
  through the lift, as an 8-bit one's already was.
- A file opens with its highlights clipped on an SDR surface. Before, a
  file with anything above white — a PQ or HLG frame, a metered scene —
  opened with the neutral curve rolling them off, and the curve was chosen
  again whenever the room changed. The curve is now only ever chosen by
  `t`, the panel's row or `--tone-map`, stays as chosen when the room
  changes, and the bottom bar's **clipped** says when there is something
  for it to do. A curve that came on by itself changed the whole picture,
  its toe darkening every shadow, for highlights a hair past white — and,
  with wide-gamut files now measured where the screen has them, would have
  come on for most phone photographs.

### Fixed

- On an HDR surface a wide-gamut color went out as a brighter color of the
  sRGB hue: the channels below zero it comes to in BT.709 were clipped
  before the surface saw them, so a P3 red was an sRGB red at 122% of white.
  They now go out as they are on an scRGB surface, and on an HDR10 surface
  the color is taken to BT.2020, which holds P3 and Adobe RGB whole, before
  anything is clipped.
- With `--output hdr` on a monitor in SDR mode, and on a press of the `HDR`
  button or `o`, a change to the room that left the surface where it was
  went unnoticed, so a gain map's lift stayed weighed for the room before.

- A panel floating over the picture could take the pointer from a place
  left of where it is drawn: the right of the histogram panel let a click or
  a drag through to the picture, and the picture beside its left edge would
  not take one.

## 0.6.0 - 2026-09-27

### Added

- The info panel reads an XMP sidecar beside the file: the file's name
  with `.xmp` in place of its extension, as Adobe's programs and digiKam
  write it, or added after it, as darktable does. A raw whose title,
  caption and keywords a cataloging program keeps in the sidecar, the raw
  itself never being written to, used to show none of them; it now shows
  them all, and the file list and the chooser find the file by that title.
  Where the file carries a packet of its own as well, the sidecar's word
  on a field is the one shown, and the packet's other fields stand.
- Every key can be rebound in the configuration file. Each has a dotted
  name — `zoom.in`, `files.undo`, `region.move.left` — and a line such as
  `keys.files.undo = ctrl+z` binds it to the keys after it, or to none. A
  key bound to one name is taken from whichever name had it, and the
  terminal says so where an earlier line had set it. The help popup, the
  tooltips, the shortcuts in the menus, and the messages about undo and
  about bringing the interface back all name the keys as they are bound.
  `--help` and the manual page show the defaults. A line left with no key
  says so in the help popup, with a warning mark and **unbound**.
- Every mouse gesture on the picture and the minimap can be given something
  else to do: `gesture.image.middle.drag = pan`, `gesture.image.middle.hold
  = loupe`, `gesture.image.ctrl+wheel = exposure`,
  `gesture.image.left.drag = zoom-box`. The wheel can step the exposure, the
  black or white point, the files or the frames, a notch at a time with a
  trackpad's scroll adding up to one, or pan, both ways on a trackpad; a
  click of any button can do what a key does.
- A double-click on the picture goes to actual size, as `1` does.
- The back and forward buttons on the side of a mouse go back and forward
  through the files that have been on screen, as `Alt+[` and `Alt+]` do.
- `--print-config` lists every key's name and every gesture at its default,
  and `--help` and the help popup gain a section for the mouse.

### Changed

- Some keys are written differently, as the configuration file writes
  them: the coordinate copy is `Ctrl+>` rather than `Ctrl+Shift+.`, the
  white point is `Shift+A` and `Shift+S` rather than `A` and `S`, and the
  previous frame is `Shift+N` rather than `N`. The keys are the same.
- `q` and `Esc` are two lines of the help, and the region's line no longer
  lists `Esc`; `x` is one line, which selects a region or removes it.
- With a region selected, the arrows move it only while the region's names
  hold them, which they do by default; bound elsewhere, the arrows pan under
  a region as they do without one.
- The file chooser's key only opens it; `Esc` or a click outside closes it,
  as a file finder does in an editor.
- The file chooser is as tall as the files that fit what was typed, its top
  staying where it is, and says **No matching files.** when none do.
- `--help` leaves a space after a key column too long for it, where
  `Alt+], Alt+Page Down` used to run into what it does.
- Every zoom works about the pointer while it is over the picture, as the
  wheel always has: the number row, `+` and `-`, the zoom menu, a
  double-click, and the actual size `Space` cycles to all keep the detail
  under the pointer where it is. With the pointer elsewhere — over a
  panel, or beside the picture — the middle of the window stays put, as
  before. A zoom to the zoom already in force — a double-click at actual
  size — moves the detail under the pointer to the middle of the window
  instead of doing nothing.

### Fixed

- A move that both pans and zooms travels in a straight line all the way,
  every point of the picture crossing the screen at a steady rate. Zooming
  out of a fit toward a detail used to pin the picture against the edge
  of the window for the first part of the move and let it catch up after,
  which bent the path.

## 0.5.1 - 2026-09-26

### Changed

- `w`, and the button beside the band under the histogram, now mark a pixel
  any channel of which has reached white or black, where they used to wait
  for all three: a sun whose red has burned out while its green never
  reached white is painted red. The test is made in the file's own
  channels, so the paint and the shares in the corners of the histogram's
  plot count the same thing, and a vivid wide-gamut color, which the file
  never clipped, is not marked for the room it lacks on the screen. A
  single-channel file is marked as before. Each pixel of the file is
  judged on its own and the marks are shrunk with the picture, so a pixel
  of the screen standing for many of the file wears the blue or the red by
  the share of them that are clipped: crushed shadows no longer sparkle as
  the picture is zoomed, go missing at 100%, or vanish as the view drops
  below it.

## 0.5.0 - 2026-09-25

### Added

- A file list. `Tab`, or the button at the head of the top bar, puts a
  strip of thumbnails down the left of the picture, one row per file in
  the order `]` and `[` walk, each thumbnail with the file's place in the
  list and its name above it — a long name cut in its middle, keeping its
  extension — and the file on screen washed in the accent. Resting on a
  row gives the folder, the type, the size in pixels and on disk, and when
  the file was last changed; sorted by anything but name, each thumbnail
  wears the value it is sorted by — for a path, its folder; a click on a
  row shows that file, and the strip scrolls to the
  file on screen as it changes. A menu at its head sorts the list by
  name, path, type, date, size on disk, width, height or area, ascending
  or descending, and the list itself is put in that order, so that the
  keys, the counter and the chooser all walk it. A sort keeps the order of
  files it cannot tell apart, so sorting by size and then by type leaves
  each type in size order, and a directory read again keeps the order it
  found the list in. A file whose header has not been read yet sorts after
  those that have, and moves into place once it is. The list opens in name
  order, whatever order the files were named in. Hiding the interface with
  `` ` `` leaves the list up without its head; `~` closes it with the
  floating panels. Dragging its right edge widens it, the thumbnails
  growing with it from 128 pixels across to 384, and what is on screen
  staying where it is. Each row is as tall as its picture's shape, from
  half as tall as it is wide to half again as tall, and square until the
  file's header has been read; the rows on screen stay put as the rows
  above them take their shapes.

- Back and forward through the files that have been on screen: `Alt+[`
  and `Alt+]`, or `Alt+Page Up` and `Alt+Page Down`, or the pair at the
  head of the file list, which are dead with nowhere to go. A step, a
  pick or a paste after going back cuts off what lay ahead, as a browser's
  history does; a file taken off the list or moved to the trash is passed
  over rather than forgotten, and is there again once undo lists it.

- `Backspace` takes the file on screen off the list without touching it
  on disk, and shows the next. The file stays off the list for the
  session however often its directory is read again; `Ctrl+Z` puts it
  back where it stood, and opening it by name again does too.

- A loupe. `l`, or the button beside the grid's in the bottom bar, rings
  the pixels around the pointer and shows them magnified in a circle
  beside it, placed up and to the right and going the other way where that
  would run it off the picture's area. Holding the right mouse button on
  the picture puts it up as well, for as long as the button is held, and
  the wheel while holding it — or `Shift+L` — steps the magnification
  through 2, 4, 8 and 16 times, the glass staying one size and the ring
  around the pointer shrinking to what fits in it, which the button reads
  out. The glass is
  the picture itself drawn again — the window, exposure, curve, false
  color and turn all reach it — and past the picture's edge it shows the
  backdrop, not the view underneath. It follows the hand through a drag.

- `Ctrl+E` exports the picture as it is shown to a new JPG or PNG beside
  the file on screen: turned, cropped to the region if one is up, and with
  the window, exposure, curve and false color written into its pixels. A
  dialog offers a name that is free, PNG or JPG with a quality slider for
  JPG starting at 90, stops a playing animation until it goes, follows the format a typed extension names, refuses a
  name already taken — nothing is ever written over — and warns of what the
  new file loses that the screen does not show: depth above 8 bits, light
  above white, transparency in a JPG, the other frames or pages, and the
  metadata. Three boxes under the formats — a percentage, a width and a
  height, locked to the picture's aspect so that typing in any one fills
  in the other two — set the size the picture is written at, from 1 to
  32768 pixels a side, the percentage counted from the region where one is
  up: a picture made smaller averages what each new pixel covers, and one
  made larger is enlarged bicubic whatever the window's own filter, which
  the dialog warns of where the window is showing nearest. The new file
  joins the list and is shown; one is written at a time, and Export waits
  for the last to land. The file menu has the item too, as Export.

- `;` and `'`, or the pair of buttons before `HDR` at the right of the
  bottom bar, turn the picture a quarter counterclockwise and clockwise. The
  turn is how the picture is shown, not
  a change to the file: it costs nothing on a large picture, carries through
  an animation's frames and a file's pages, is kept with the file for the
  session as the exposure is, and turns a region with it.

- An iPhone's HDR photograph arrives as HDR. A HEIC carries its gain map
  beside the picture — in ISO 21496-1's standard form from iOS 18 on, and
  in Apple's own before — and either is read and applied the way an Ultra
  HDR JPEG's is. `--no-gain-map` leaves the map unread, as it does for a
  JPEG.

- The orientation tag is applied in every format that carries one. A
  JPEG's EXIF orientation, a TIFF's `Orientation` tag and a PNG's `eXIf`
  chunk join WebP, HEIF and JPEG XL: a photograph taken with the camera on
  its side arrives upright, as it does in a browser, and the window opens
  in the shape it will arrive in. An Ultra HDR file is turned the same
  way, and so is every frame of an animated PNG and every page of a TIFF.
  The preview a raw carries of itself is left as stored and turned by the
  camera's own orientation, as before, so a preview that repeats the tag
  is not turned twice.

- ProPhoto RGB is recognized, from a profile's colorants or a PNG's
  chromaticities, and `--primaries prophoto` names it: a 16-bit export from
  Lightroom, which edits in ProPhoto and writes it by default, arrives with
  its colors rather than the flat ones sRGB primaries make of them. The
  gamma Adobe's own profiles state — 2.2 for Adobe RGB, 1.8 for ProPhoto —
  is read as well; before, both were read with the sRGB curve, which put
  Adobe RGB's shadows a little wrong and ProPhoto's a lot.

- A TIFF's embedded ICC profile is read, through the same reader JPEG,
  PNG, WebP and HEIF's go through, and it settles what the file means: a
  16-bit export from Lightroom or Photoshop no longer arrives as linear
  measurement data to be windowed, but as the Adobe RGB or Display P3
  picture it was graded as. Without a profile the reading by depth stands.

- A PNG's `gAMA` and `cHRM` chunks are read where it carries neither code
  points, a profile nor an `sRGB` chunk: a gamma of 1.0 marks the file as
  linear light, as a renderer writes it, and primaries stated as
  chromaticities are matched against the four the program can name.

- The program opens with nothing: `gamut` alone, or from the desktop's
  menu, puts up an empty window with three buttons in the middle of it —
  **Open files…**, **Open folder…** and **Paste** — and comes back to them
  when nothing it was handed could be opened, with the reason at its foot.
  `Ctrl+O` puts up the desktop's own file dialog from any window, narrowed
  to the formats read here, and `Ctrl+Shift+O` the same dialog for a
  folder; what is chosen joins the end of the list as if it had been named
  on the command line after the rest, and the first of it is shown. The dialog
  is asked for through the desktop portal over a session-bus connection of
  the program's own, so nothing new is linked. `--paste` with nothing on
  the clipboard now opens the empty window rather than failing. The first
  picture to arrive in an empty window sizes it as the window would have
  opened on that picture, unless `--size` chose the size.

- A configuration file, `~/.config/gamut/config`, saying how the window
  opens: whether the interface, the minimap, the file list, the histogram
  and the file information are up, how the pixel under the pointer is
  read out, and whether the histogram's counts are logarithmic. A flag
  for the same panel wins over it; a line it cannot use is named on the
  terminal and the rest still taken. `--print-config` prints one with
  every setting commented out at its default, to start from; none is
  written for you. See `user-docs/SETTINGS.md`.
- The file list's width and the loupe's magnification are remembered from
  one run to the next, in `~/.local/state/gamut/state`, written when the
  window closes.

### Changed

- The file list is up when the window opens, and the pixel under the
  pointer is read out in hex. The configuration file puts either back.
- The name of a file deleted while it is on screen is struck through in
  red, in place of the word `DELETED` beside it.
- A file that takes more than a quarter of a second to open says so in a
  toast, `Loading` and its name — `Reloading` for the file on screen read
  again — that stays up until it opens, in place of the words after the
  name in the top bar. Stepping on while it is up keeps it up, naming the
  file now being read.

- `Backspace` no longer moves the file on screen to the trash; `Delete`
  alone does. The undo key's line in the help says it undoes a removal
  as well.

- The information panel's "Read by" line, and the file list's type, name
  the format a file turned out to be rather than the decoder that read
  it: a GIF says `gif` and an EXR `exr` where both said
  `gif/hdr/exr/bmp/netpbm`, an AVIF says `avif` and a HEIC `heic`, and a
  camera raw says which — `nef`, `arw`, `cr2`, `dng` and the rest — where
  its bytes say, and `camera raw` where they do not.

- A new icon: a lake at sunset, with a loupe over the sun's rim showing
  its pixels. `bin/icon` draws it from a model of the scene's light.
- The histogram's logarithmic count axis is toggled by `y` rather than
  `l`, which the loupe has taken.

- A gain map is applied as far as the monitor has room, as the standard
  says and as a phone's gallery does, rather than the whole way and then
  tone mapped. On a monitor in SDR mode a photograph with a gain map — an
  Ultra HDR JPEG, an iPhone's HEIC — now shows exactly as the phone graded
  it, where before its lifted midtones came out brighter than graded and
  its highlights piled into the tone curve's shoulder; on a monitor in HDR
  mode the highlights are lifted as far as the monitor's headroom allows.
  The map goes to the GPU beside the picture and is applied there, so
  switching the room with `o` is instant, the histogram, the readout and a
  copy describe what is on screen, and a 24-megapixel photograph takes a
  quarter of the memory it did.

- Deleting the only file on the list takes it off the list and off the
  screen, leaving the empty window and its buttons, rather than keeping it
  up marked `DELETED`. Undo puts it back and shows it, as it was left. A
  file whose neighbor will not open still stays up, marked, as before.

- A menu of the file itself, off a button before its name in the top bar:
  copy its name, copy its path, copy its URI, rename it, delete it. `F2` opens the rename
  dialog, which says what is wrong with the name as it is typed — taken, a
  slash — and notes an extension that changes; `Delete` or `Backspace` moves
  the file to the desktop's trash, where the file manager shows it, and
  shows the next file. `Ctrl+Z` undoes either, back through the session:
  the file comes out of the trash and back into the list, or gets its old
  name back.

- `Ctrl+V` with no image on the clipboard says so in the window, where
  before it said so only on the terminal.

### Fixed

- A gain map is turned with its picture. A JPEG with a gain map and an
  orientation tag other than upright had its lift applied to the wrong
  pixels on an HDR monitor, since the map was left as stored.

- The rounded corners of the icons — the folder, the sheet, the clipboard,
  the hook of the question mark, the ring of the reset arrow — came out
  gray beside their sides. Each curve was drawn as a chain of short
  strokes, and egui feathers every stroke on its own: a feather laid over
  ink already there dims it, so a curve that was nothing but overlaps was
  dim all the way round. A curve is now one stroke from end to end, and at
  a stroke a single pixel wide — a small icon on a monitor at scale 1 — its
  points are put on the pixel grid as the straight strokes' are, since a
  one-pixel curve left where it falls comes out at half weight.

## 0.4.0 - 2026-09-21

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

## 0.3.1 - 2026-09-20

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
  in the list with `:12`, or from its end with `:-1` — arrow through the rows, and `Enter` or a click
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
