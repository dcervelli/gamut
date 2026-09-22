# gamut

A GPU image viewer: Rust, `winit`, `wgpu`, `egui`. `docs/` explains the
design and the reasoning behind it; this file is the map for making changes.

## Layout

Dependencies point downward only. Nothing in a lower layer imports a higher one.

```
main.rs        mod decls, event-loop bootstrap
cli.rs         argument parsing; usage() renders the key sections from app::input::KEYS
app/           the event loop's state and winit handlers
  mod.rs         App: window, renderer, loader, apply/deliver/redraw, ApplicationHandler impl
  animation.rs   Animation: the animation on screen — the thread decoding its frames, its clock, and
                 which frame the texture holds — one without the others never being the case
  gui.rs         Gui: egui's context and its winit adapter; runs a pass, merges
                 when egui wants painting again into the loop's deadline
  files.rs       Files: the file list, the read in flight, walks past broken files (pure, tested)
  kept.rs        what each file was left in — its view, its display, and its frame or page — so stepping back to it puts it back
  edits.rs       what is done to the file on disk — moved to the trash, renamed — and the
                 stack that undoes it; the rename dialog's state while it is up
  playback.rs    the clock an animation plays by: which frame is due at a moment, and when the next is;
                 pure, told the time and the delays decoded so far
  region.rs      Marking: the region marked out on the picture as the application holds it — what it
                 is in, the handle the arrows move, the hold a drag has on it, what Space frames
                 next, the box being dragged out to zoom to — and the gestures, in image pixels
  chooser.rs     the file chooser's state: the query, which files fit it and where, the
                 cursor, what is known about each file, and the thumbnails the screen holds
  copying.rs     Copying: the copies of the picture being prepared on threads of their own,
                 each with the Ticket it reports through and tells whether it was superseded by
  input.rs       Action, KEYS table, Effect; perform() is where every key's action happens;
                 act() is where every Command from the interface happens; Namer composes tooltips from KEYS
  window.rs      opening size, titles
ui/            lays each frame's interface out with egui; no wgpu or winit imports
  mod.rs         Current, Panels, FrameInput, show() — the whole interface, handing back Commands —
                 the picture's own drag and wheel, room(), backdrop()
  control.rs     Control (everything that can be pressed), Command (what a pass asked for),
                 Naming (the words the application has for the interface)
  chrome.rs      the four panels as egui Panels and everything on them; Pass is one pass of the
                 interface — what it is drawn from, and the device's grid, the content area and
                 the room it has that every widget reads rather than works out again — with
                 icon_button(), button_ink() and tooltip(); content_area(), image_viewport() —
                 pure geometry, worked out before egui lays anything out
  style.rs       Theme's roles as egui's Style and Visuals; Color into Color32
  fonts.rs       the desktop's sans, bold and monospace faces, as fontconfig resolves
                 them, each with its capitals centered in egui's rows
  rect.rs        Rect, the logical-pixel rectangle the panels are placed by
  panel.rs       where a thing floating over the picture goes — fit(), the one placement every
                 panel is fitted by, refused rather than shrunk — and area(), the one opening
                 every panel makes
  histogram/     the histogram panel: mod.rs its geometry, words, header and rows, plot.rs the
                 plot, track.rs the band and its handles, controls.rs the buttons, slider.rs the
                 exposure's slider; handle() in mod.rs is the one handle the band and the slider draw
  minimap.rs / grid.rs   one widget each, drawn with egui's painter
  region.rs      the region marked out on the picture: where its outline and
                 eight handles go, which handle the pointer is on, and the
                 words it wears while the pointer is on it — its size at its
                 middle, each edge's coordinate inside that edge's mark, and
                 labels() deciding which of them there is room for; the
                 gestures on it are read in Pass::picture against the same
                 geometry
  icon.rs        the marks a button wears: Lucide's geometry on its own
                 24-unit grid, sized and placed in whole device pixels so
                 strokes stay sharp and evenly spaced marks stay even; Grid is
                 the device's grid, paint() draws through egui's painter
  info.rs        the file's own facts, in a scroll area, each block a press that copies it
  pixel.rs       the pointer's readout: coordinate, swatch, and the pixel's
                 value in whichever of `PixelFormat`'s three ways is in force
  menu.rs        the popup menus' contents: the zoom menu's choices and cells, the
                 pixel-format cells, and the menus of copies and of the file with each item's key beside it
  rename.rs      the rename dialog: a modal with the name in a field, judge() saying what is
                 wrong with what has been typed as it is typed, and OK and Cancel
  tooltip.rs     the label naming what the pointer is resting on: Tip is what can
                 have one, Tooltip is what is said, disabled() why a dead control is dead;
                 when it opens and where it goes are egui's
  toast.rs       the message about what was just done, at the foot of the content
                 area: Toast is what it says and how long it has, Toasts is the
                 clock, and show() draws it with the cross that dismisses it
  chooser.rs     the file chooser's popup: the field, the rows and their thumbnails, and
                 the keys read inside the pass while the field has the keyboard
  empty.rs       what the content area shows with nothing open: the buttons that put up
                 the desktop's file dialog, for files or a folder, and the paste
  help.rs        the help popup: the key table laid out in three columns — the keys, what
                 they do, and when — from the Sections and Rows Naming::help hands over
  transport.rs   the bar a file of frames or pages brings with it, above the
                 bottom bar: the steps, the play button, the readout and the
                 timeline; Transport is what it shows, and timeline() lays the
                 track out in time
  status.rs      the words in the top and bottom bars: top_words() and state_words()
                 lay them out, state_words() naming what is being done to the
                 picture, and explain_state() says the same in sentences for the tooltip on it
  driven.rs      (tests) the interface driven headless through egui_kittest
theme/         palette.rs reads Omarchy's colors.toml and resolves its cascade; mod.rs derives Theme's color roles
view.rs        zoom / pan / fit geometry, pure maths (View, Viewport, Fit); Position is
               the view in space-scale coordinates, where the line a move follows is straight
motion.rs      a pan or zoom on its way: where the view was shown when it was asked to
               move, and how far along it is; where it is going is App's View itself
listing.rs     what a path on the command line stands for: a directory is the
               images inside it, read again while the program runs
loader.rs      the decode + upload thread; replies arrive as winit user events; a
               paste is fetched here too, on its way to being read; a page of a
               paged file is a read like any other
player.rs      the thread decoding an animation's frames ahead of the clock, and
               the cache it keeps them in under MAX_SEQUENCE_BYTES: whole for a
               file that fits, a window around the head for one that does not
thumbnailer.rs the low-priority thread thumbnailing every file of the session for
               the chooser, in two passes — every header and title first, then the
               thumbnails — and the queue the chooser's visible rows go to the front of
thumbnail.rs   the freedesktop thumbnail cache: the GLib-spelled URI a file is keyed
               by, MD5, the chunks a thumbnail carries, and the temporary-then-rename write
fuzzy.rs       the chooser's matcher behind a trait with skim's own signature; the
               one file that names the fuzzy-matcher crate
watch.rs       polling a file — or a directory — for a settled change
trash.rs       the desktop's trash, by the freedesktop specification: put() moves a file
               into it and says which Entry it became, restore() moves that entry back
monitor.rs     what the compositor says each monitor is, SDR or HDR and how large in logical pixels, over a Wayland connection of its own
clipboard.rs   putting text or a file: URI on the clipboard, in a process that outlives
               the window; and reading a pasted picture off it
pasted.rs      where a pasted picture is written and what it is called: the
               XDG pictures directory, and a name nothing else holds
portal.rs      the desktop's file dialog, through the file chooser portal: Pick says
               files or a folder, choose() blocks for the answer, choose_on_thread()
               hands it back through the event loop
dbus.rs        the session bus, spoken directly: Value marshaled and unmarshaled by
               signature, and a blocking Connection that calls a method and waits for
               a signal — enough for the portal and nothing more
openers.rs     what else on the desktop can open the file on screen: the entries
               that claim its MIME type, found through the desktop's own index and
               the user's associations, and starting one of them
clock.rs       a moment as a date and time — UTC, or the zone the system's own
               compiled zone file says it is in
timing.rs      startup instrumentation
image/         the data model, nothing GPU
  mod.rs         Channels, Samples, AlphaMode, Referred (graded or measured light), DecodedImage,
                 Sample (one pixel read back)
  color/         Transfer, Primaries, ColorSpace; icc.rs and cicp.rs translate what files say into them
  region.rs      Region, a rectangle of the image's own pixels, and every
                 pure change to one: drawn, moved, pulled by a handle, grown,
                 nudged
  sequence.rs    what a file holds beyond one image: Sequence (still, animation,
                 pages), Frame, and the FrameSource a decoder's frames come
                 through, each composited whole by the decoder
  stats.rs       the scan an image gets on load: min/max, histogram, plot; scan_with lifts a gain-mapped picture as the screen shows it
  gain_map.rs    a gain map beside its SDR base: what its values mean (ISO 21496-1's or Apple's Lift), the
                 weight the display's room gives it, the Table a weight makes, and gain_at, the shaders' twin
  encode.rs      the displayed image walked back out to an 8-bit sRGB PNG, for the clipboard
  resample.rs    a CPU box filter in the file's own encoding, for the thumbnails
  exif.rs        the file's own metadata, read and rendered for the info panel: the EXIF
                 block, and the XMP packet's words merged into its About section
  xmp.rs         the XMP packet: found in each container by walking its headers, and
                 parsed into namespaced properties; which of them are shown is exif.rs's
  geo.rs         GeoTIFF's keys: where a raster's pixels are on the ground
  isobmff.rs     the ISO base media boxes, walked in memory: what a HEIF's meta box, a CR3's
                 moov and a gain-map item are all read through
  tiff.rs        what every reader of a TIFF-shaped block shares: the four signatures, the
                 prefix read of one, the byte order and its numbers, a directory's entries and
                 the tags that point at another directory
  directory.rs   a TIFF directory the metadata reader cannot reach, rewritten
                 as a block it can: BigTIFF, or a directory past the prefix
  enclosed.rs    where a raw that is not a TIFF at the front keeps its EXIF:
                 an ORF or RW2 under its own magic, the JPEG in a RAF, the
                 TIFF in an MRW, the boxes of a CR3 written back out as one
  display/       Display: window, exposure, tone map, colormap — uniform state, never re-decodes;
                 mod.rs is the state and map(), auto.rs the window rules, tone_map.rs the
                 curves (the CPU twins of the compositor's) and Headroom, colormap.rs the
                 ramps the image layer writes its texture from
  decode/        Decoder trait + DECODERS registry in mod.rs; one file per format; limits.rs the size ceiling;
                 dynamic.rs the shared DynamicImage bridge; orient.rs the turn an orientation tag asks
                 for, applied to any layout; heif/tmap.rs reads ISO 21496-1's gain-map item and
                 heif/apple.rs Apple's maker note; fixture_tests.rs runs every file in test_images/
render/        the GPU
  mod.rs         Renderer: surface, device, the three passes; Scene is what a frame draws;
                 UiPaint is what egui drew, and the ui-layer pass hands it to egui-wgpu
  color.rs       Color, the sRGB color the interface and the theme speak in
  placement.rs   Placement (where the image lands) and Upscale (the magnification filter)
  upload.rs      texture format choice and the transfer-function LUTs; the "sampled texel is linear" invariant
  image_layer.rs / reduce.rs / composite.rs   the passes
  shader_codes.rs  every Rust<->WGSL integer code, one fn per shader switch
  gpu.rs         wgpu boilerplate helpers (layouts, uniform buffers, full-screen pipelines)
  shaders/       WGSL; each Params struct is mirrored by a #[repr(C)] struct in the .rs file that
                 loads it; texel.wgsl is the reading of one texel that mod.rs prepends to
                 image.wgsl and reduce.wgsl both
packaging/     what an Arch package is built from: PKGBUILD, the .desktop entry,
               the icon, and hand-written shell completions
bin/           release, and pkgbuild-sha which points the PKGBUILD at a published tag
REUSE.toml     which file in the tree is under what license; LICENSES/ holds the
               texts it names, and `reuse lint` checks the two agree
about.toml     which licenses a dependency may arrive under; packaging/about.hbs
               renders THIRD-PARTY-NOTICES, the notices of every linked crate,
               which bin/release regenerates and stamps with the lock it read
```

The program answers to two names, both in `main.rs`. `PROGRAM` is the crate's
own name, and what a user types: the binary, the window title, the Arch
package, the man page and the completions. `APP_ID` is `com.dcervelli.gamut`,
what the desktop knows it by: the Wayland `app_id` and the X11 `WM_CLASS`, and
so the basename of the desktop entry and of the icon, which the entry repeats
in `Icon=` and `StartupWMClass=`. `cli.rs`'s tests check that `packaging/`
still agrees with both, so renaming either is editing the constant —
`Cargo.toml` for `PROGRAM` — and moving the files those tests name.

## Where to make a change

| Change | Edit |
| --- | --- |
| A key binding | `app/input.rs`: one `KEYS` entry, with the `mods` it is held with and a `when` if it only does anything under some condition, and one `perform` arm. `--help`, the man page and the help popup follow. A condition new to the table is a `When` variant, its words in `When::describe`, a field of `Conditions` and its reading in `App::conditions`, from the same state the `perform` arm reads. `Conditions` is also what makes a control dead — `Conditions::reasons` is the tooltips' reading of it, and `App::refuses` the press's — so a button drawn dead, its label and its press cannot disagree |
| A button | a `Control` variant in `ui/control.rs` with its `label`, the widget where it is drawn — `Pass::icon_button` for a square toggle — pushing `Command::Press` on a click, and an arm of `App::press`. Keys that do the same job go through `press` too, so the two cannot drift apart. What it says when rested on is an arm of `ui/tooltip.rs::words` or of `input::action_of`, both exhaustive, so a nameless button does not compile; and one of it goes in `Control::ALL` for the tests that ask something of every button |
| A status-bar segment | `ui/status.rs`; the pointer's pixel readout is `ui/pixel.rs` |
| What a file is left in when you step off it, and what comes back when you step on to it | `app/kept.rs`, and the arrival in `App::apply`, which trades the outgoing file's settings for the incoming one's by what `Arrival` the file is — read again at its size or at another, another file beside one of the same size, or one arriving anew — as `arrival()` names it |
| Whether a change to the view is a move or a cut | `App::animate` around the change, in `app/input.rs`, makes it a move; a change the hand is on — a drag, a single pixel's step, a trackpad's scroll — goes to `App::view` directly. Whatever reads what is on screen reads `App::shown_view`, not `view`; how long a move takes is `motion::DURATION`. The drag and the wheel themselves arrive from `ui::show` as `Command::Drag` and `Command::Wheel`, off the picture's own response in `Pass::picture` |
| What a pixel reads as under the pointer | `image/mod.rs::sample` for what the file holds, `image/display/mod.rs::map` for what the screen shows, `ui/pixel.rs::PixelFormat` for which of the two the bar writes out and how |
| What the window says about something that just happened | `ui/toast.rs` for how long it stays and how it is drawn, `App::toast` to raise one, and `App::poll_copies` for the copies that only know how they went once their thread is done |
| What something is called when the pointer rests on it | a `Tip` variant in `ui/tooltip.rs` and one `pass.tooltip(response, tip, enabled)` on the widget's response; `Namer::tooltip` in `app/input.rs` composes the words, from `KEYS` by way of `action_of` wherever a key does the same job, so a tooltip and `--help` cannot disagree. Words of its own go in `ui/tooltip.rs::words`, a zoom cell's in `ZoomChoice::describe`. When it opens and where it goes are egui's, tuned in `ui/style.rs` |
| A button's icon | `ui/icon.rs`: one `&[Mark]` on the 24-unit grid, and one `icon::paint` call where the button is drawn, in a square from `icon::square`. The caller sets aside a budget; whether the mark comes out sharp is `icon::Grid`'s business and whether its spacing stays even is `icon::fit`'s |
| A panel or overlay | a new `ui/<name>.rs` with a `show(pass, ui, ..)` that opens `panel::area` at the rectangle its own `panel()` works out through `panel::fit` from `pass.content`, and one call in `Pass::overlays`; the picture is `pass.current`, the device's grid `pass.grid`. An area that is `interactable` takes the pointer from the picture under it; `Order` is the height in the stack. A new color role goes in `theme/mod.rs` and, if a stock widget wears it, `ui/style.rs` |
| What the info panel says about a file | `ui/info.rs` for the layout; the file's own facts are gathered in `app/mod.rs::file_facts`, its metadata in `image/exif.rs`, and its georeference in `image/geo.rs`. A field written in words is a `Described` entry in `image/exif.rs`, naming its EXIF tag and its XMP property; a container's XMP packet is found in `image/xmp.rs::packet`. A raw's `Sensor` section, and the exposure of one whose EXIF says nothing, are what LibRaw read, handed over by `Decoder::facts`; a container whose packet only its own library reaches hands that over by `Decoder::xmp`; a raw container's EXIF block is found by `image/enclosed.rs` |
| A thumbnail's source | `Decoder::preview` for a format that carries a smaller picture of itself, which the thumbnailer asks for before it decodes anything; `thumbnail::SIDE` is the size it has to reach to be used |
| A popup menu | a function in `ui/menu.rs` that lays its cells out, each pushing `Command::Press` of a typed `Control` — `ZoomTo`, `Format`, `Copies`, or `Opener`, which is a place in a list the application built rather than a choice named in the source — and an `egui::Popup` hung off its button in `ui/chrome.rs`, aligned below, above or beside it with `RectAlign`. The popup opens, closes and takes the pointer by itself; `App::close_menus` is how a key closes one. An item that does something rather than setting something is performed in `App::press`, as the menu of copies is, and prints its key beside it from `Naming::shortcut` |
| What else can open the file on screen, or what happens when one is chosen | `openers.rs`: `MIME_TYPES` says what the desktop calls a file this program reads, `read_entry` which entries are offered and which are left out, and `open` how one is started. The list is read once per file in `App::apply`; the button is `Pass::open_button`, the menu `ui/menu.rs::open_items`, and the press `App::open_in` |
| A CLI flag | `cli.rs`, and the `Options` / `Startup` / `Overrides` field it sets |
| What the empty window offers, or what the file dialog asks for | `ui/empty.rs` for the buttons and where they go; `portal.rs::choose` for the dialog's title, filters and options, `Pick` for which kind; `App::pick` puts it up and `App::picked` takes the answer, `App::open_named` opens what was chosen as a command line naming it beside the rest would — `listing::expand`, then `Files::append`, the newcomers at the end of the list and the first of them asked for as a walk. `App::is_empty` is what puts the buttons up, `App::leave_picture` is how the last deleted picture comes down into it, and `App::size_to_next` is what has the next picture size the window; `App::from_command_line` is whether nothing showing means leaving |
| Another call to the desktop over D-Bus | `dbus.rs`: `Connection::call` for a method, `add_match` and `wait_signal` for a signal, `Value::dict` for an `a{sv}` of options; a new basic type is an arm of `Value` and of the writer and reader alike |
| A picture in the README or `user-docs/` | not kept here: the pictures, and the scripts that take them, one per picture, are in the `gamut-scripting` repository beside this one, in its `screenshots/`, and the README links to them there by absolute URL. Run the picture's script again after the interface changes rather than retouching the picture, and push it in that repository |
| What a path on the command line stands for | `listing.rs`; `App::poll_directories` notices a named directory changing and `app/files.rs::relist` takes the new list in |
| Anything a user installs — the desktop entry, the icon, a completion, the man page | `packaging/`; the man page is generated by `cli.rs::man()` from the same `OPTIONS` and `KEYS` as `--help`, so it is never edited directly |
| An image format | `image/decode/<fmt>.rs` implementing `Decoder`, one line in `DECODERS`, a fixture in `test_images/` (see its README and `generate.sh`); a camera format is LibRaw's to read and `decode/raw.rs`'s to recognize, checked against a real file by `test_images/raw-samples/fetch.sh` |
| A format's frames or pages | `sequence` on its `Decoder`, answered from the header, and `frames` (a `FrameSource` handing over each frame whole, at the canvas size) or `decode_page`; a fixture that is the pattern followed by the pattern upside down, and an entry in `fixture_tests.rs::SEQUENCES` |
| How an animation is timed, cached or shown | `app/playback.rs` for when a frame is due; `player.rs` for what is decoded ahead and what is let go; `app/animation.rs` for the pair as the application holds them, `Animation::due_frame` handing over the frame that should be up and `App::show_due_frame` putting it in the texture and `Current`; the display is left alone on a frame change on purpose |
| What the transport bar shows or does | `ui/transport.rs` for the bar and `App::transport` for what it is told; a press goes through `App::press` as `Control::{Play, StepBack, StepForward, Seek}`, and the keys through `App::step_frame` and `App::toggle_play` so the two cannot drift; `Chrome::new`'s flag is where the bar takes its height |
| What a copy of the image contains | `image/encode.rs`, which walks the `Region` it is given; the chord that asks for it is in `app/input.rs` |
| The chooser: what a row shows, how the query is matched, what a key does in it | `ui/chooser.rs` for the popup and the keys it reads before its field can; `app/chooser.rs` for the ranking, the relative paths, the title beside them (`candidate`) and the cursor; a new fact for a row is a field of `thumbnailer::Facts`, read in `thumbnailer::header`; `App::press` for `Control::Chooser` and `Control::Choose`, and `App::act` for `Command::{Query, Cursor, Visible}`. Its open state is egui's, under `ui::chooser::id()`. A different matcher is a new `impl Matcher` in `fuzzy.rs` |
| A thumbnail: what is made, where it goes, what the row gets | `thumbnailer.rs` for the stages and the queue, `thumbnail.rs` for the cache's naming, chunks and write; `image/resample.rs` for the filter; `App::hold_thumb` for the texture and `app/chooser.rs::Thumbs` for how many the screen keeps |
| What a region does, or what a key does while one is up | `image/region.rs` for the change to the rectangle; `app/region.rs::Marking` for what the application holds about it and what a drag makes of it (`grab`, `pull`, `release`), tested with no picture; `App::perform_on_region` in `app/input.rs` for the keys a region takes; `ui/region.rs` for where it is drawn and which handle the pointer is on; `Pass::region_gestures` in `ui/mod.rs` for which drag is the region's and which the view's |
| What deleting or renaming a file does, or what undo puts back | `app/edits.rs`: `App::delete_shown` moves the file to the trash through `trash::put` and steps away, `App::rename_shown` renames through `trash::rename_no_replace`, and each pushes an `Edit` that `App::undo` pops. The list's side is `app/files.rs`: a trashed file is `condemn`ed and leaves in `shown` — or, the last on the list, in `remove_shown` at once — comes back by `reinstate`, and a renamed one is `rename`d in place. The dialog is `ui/rename.rs` — its words, and `judge` for what is wrong with a name — with `App::set_rename_name` asking the directory whether the name is taken. The menu is `menu::file_items`, hung off `status::file_button` |
| What a paste accepts, or where it is written | `clipboard.rs::IMAGE_TYPES` for the MIME types and the extensions they are saved under, `pasted.rs` for the directory and the name; `App::paste` starts it and `app/files.rs::adopt` puts it in the list. Whether the button for it is on screen is `Panels::paste`, looked at by `App::poll_clipboard` |
| A color-space source (a new tag a format carries) | `image/color/` |
| Someone else's work brought into the tree | say where it came from beside the code that carries it, then one `[[annotations]]` entry in `REUSE.toml`; if its license is new to the tree, its text goes in `LICENSES/` named by SPDX identifier, and the PKGBUILD's `license=()` grows an entry |
| A dependency | `Cargo.toml`, then `bin/release` rewrites `THIRD-PARTY-NOTICES`. A license `about.toml` does not accept fails generation: add it there, in priority order, and its text to `LICENSES/`, or take the dependency instead |
| An upscale filter or tone map | the WGSL function, one arm in `render/shader_codes.rs`, one enum variant with its `label`/`parse`/`next`; a tone map's CPU twin is `ToneMap::apply`, which takes the surface's `Headroom` as the shader arm does |
| How far a gain map lifts the picture, or what reads through the lift | `image/gain_map.rs`: `GainMap::weight` for the share of the lift a display's room gets, `GainMap::table` for what a weight makes of the map, `gain_at` for the CPU twin of the shaders' `gain` — keep the three in step. `App::display_headroom` is what the weight is fed, `App::refresh_lift` puts the table in `Current::lift` and scans the statistics through it, and `Scene::lift` carries the weight to `image_layer`, which writes the table to the device and rebuilds the coarse chain. Whatever reads a pixel takes the table: `DecodedImage::sample`, `encode::displayed`, `Stats::scan_with` |
| What the surface can be, SDR or HDR | `render/output.rs` chooses it; `monitor.rs` says what the monitor is in; `App::surface_hdr` and `App::headroom` put the two together, `App::sync_output` acts on them, and `App::toggle_hdr` is what the bar's `HDR` button and `o` both call |
| A new render pass | build it from `render/gpu.rs`; add its target to `Renderer::render` |
| Something about the display window, exposure or false color | `image/display/` (state) and `shaders/image.wgsl` / `composite.wgsl` (effect) |
| What the histogram panel's rows hold, what a drag on its band does, or what its corners say | `ui/histogram/mod.rs::Rows` for the three rows, which are every file's; `ui/histogram/track.rs` for the band and its handles, which ask through `Command::{BlackPoint, WhitePoint, Slide}` and land in `Display::put_black`, `Display::put_white` — each its own end of the window, the exposure left alone — and `Display::set_displayed_bounds`; the keys that step the handles are `Action::{StepBlack, StepWhite}`, landing in `Display::step_black`, `Display::step_white`, by `input::WINDOW_STEP`; `ui/histogram/slider.rs` for the exposure, which asks through `Command::Exposure` and lands in `Display::set_exposure`, `SLIDER_STOPS` being how far it runs; `Plot::clipped` for the shares in the plot's corners, and `Display::clips_white` for whether white counts. The marks `w` and the button beside the panel's band paint on the picture are `marks` in `shaders/image.wgsl`, `shader_codes::marks`, and `Panels::mark_clipped`, toggled in `App::press` by `Control::Marks` |

## Conventions

- Decoders describe, they do not normalize: keep 16-bit and float data as it
  is and say what it means through `ColorSpace` and `AlphaMode`.
- A sampled texel is always linear in the working space. Transfer functions
  are resolved once at upload (`render/upload.rs`), never in a shader.
- The interface is laid out in logical pixels; the image is placed in
  physical ones. `FrameInput.scale` converts. A display need not have a whole
  number of device pixels to the logical one, so anything thin — a rule, an
  icon's stroke — is put on the device's own grid before it is drawn
  (`icon::Grid`'s `snap`, `rect`, `line_width` and `stroke_center_in_device`).
  Rounding to a whole logical pixel is not the same thing and is not enough.
- The geometry the picture is fitted into is worked out before egui lays
  anything out (`chrome::content_area`, from the window size alone), and
  egui's panels are given exactly those sizes: a fit that waited on the
  toolkit would be a frame behind the window.
- The interface never acts on the application. `ui::show` hands back
  `Command`s, and `App::act` does them after the frame is off, through the
  same `press` a key goes through. A widget that needs words asks the
  `Naming` it was given.
- Every control senses `Sense::CLICK`, never `Sense::click()`: the latter
  takes keyboard focus on a press, and a focused button would swallow every
  key after it. Keys stay on winit's table, and go to egui only while
  `text_edit_focused()`.
- Stock egui widgets wear the theme through `ui/style.rs`; anything drawn by
  hand reads its inks from `Theme` directly, through `Pass::button_ink`.
- Text laid out to be drawn is laid out in `Color32::PLACEHOLDER`, and its
  ink goes to `Painter::galley`. A color given to `layout_no_wrap` is baked
  into the galley and the painter's is then ignored, so a galley laid out in
  white stays white however the theme changes — which looks right on a dark
  theme and is unreadable on a light one. Laying it out in the ink itself is
  the other correct form, as `ui/info.rs` and `ui/status.rs` do; what must
  not happen is one color in the layout and another at the painter.
- The interface's face has no U+2192, and the arrow the fallback supplies sits
  low, so a readout showing one thing become another uses `ui::BECOMES`.
- Handlers return an `Effect` (`Redraw` / `Nothing` / `Quit`), never call
  `request_redraw` themselves: `App::settle`, called last by each winit
  handler, is the one place a frame is asked for, and two effects fold
  with `Effect::also`. A helper that changes what is on screen returns an
  `Effect` too, so that the caller cannot forget the frame it owes.
- Docs: three directories, three audiences — see **Docs ownership** below.
- American spelling throughout — code, comments, docs, and every word a user
  reads: `color`, `gray`, `center`, `normalize`, `license`, `behavior`,
  `canceled`. The tree holds no British spelling of any of them, so a search
  for one finds every use rather than half of them. Two words are kept in
  their British form on purpose, both of them things a user might type at us
  rather than read from us: `--colormap grey` is taken as `gray`, and the
  desktop entry's `Keywords=` lists `colour` beside `color` so that a search
  in either finds the program. The other exception is a name someone else
  chose, quoted as they wrote it: a spec's own wording, an external API, an
  SPDX license identifier.

## Docs ownership

Four places, and a fact belongs in exactly one of them. Getting this wrong is
how the README turned into a 1200-line specification once already.

- **`README.md` is human-authored. Do not edit it.** It is the front door: what
  the program is, who would want it, how to install and start it, and what it
  cannot do yet. If a change makes the README wrong or incomplete, say so in
  the summary of the change and leave it alone — do not fix it, do not extend
  it, and never add a section describing something newly built.
- **`user-docs/`** is for people who only run the program. It has its own
  CLAUDE.md with the rules for writing it.
- **`docs/`** is for people reading or changing the code: the design and the
  reasoning behind it, as the code works now. It has its own CLAUDE.md too.
  This is where implementation writing goes — new pages here, not new README
  sections.
- **`CHANGELOG.md`** takes anything of the form "what is new" or "what changed".
  Nothing of that shape goes in the README or in `docs/`.

`CLAUDE.md` files, this one included, are conventions rather than explanation:
what a change must do, not why the code is shaped as it is. `notes.md` is the
author's own roadmap — read it, do not write to it.

## Checks

```sh
cargo test
GAMUT_REQUIRE_GPU=1 cargo test render::   # the GPU tests, made to fail rather than skip
cargo clippy --all-targets    # clean
cargo doc --no-deps           # no warnings
cargo run --release -- test_images/png-rgb8.png --histogram
cargo audit                   # no advisories against the pinned dependency versions
reuse lint                    # every file accounted for by REUSE.toml
cargo about generate --frozen packaging/about.hbs   # only after a dependency changes
```

Decoders parse untrusted files, so treat a panic on a malformed input as a
bug: the loader turns one into a recoverable decode error, but every decode
path should be size-checked before it allocates, and a new format wants a
malformed fixture as well as a valid one.
