# macOS

The same program on a Mac, with the desktop's services spoken to through
AppKit and Foundation. What the keys, the files and the paths are for a user
is in [`user-docs/`](../user-docs/); this page is how the platform is split
off and why each Mac half is shaped the way it is.

## Where the platform is

Each module that talks to the desktop is a directory. What every platform
shares — the types the application reads, the tables, the thread that
watches the clipboard — is in its `mod.rs`, and each mechanism is a file
beside it, chosen by `#[cfg(target_os)]` and re-exported under the same
names:

| Module | Linux | macOS |
| --- | --- | --- |
| `clipboard/` | `wayland.rs`: the selection, held by a second process | `macos.rs`: `NSPasteboard` |
| `portal/` | `freedesktop.rs`: the file chooser portal over `dbus.rs` | `macos.rs`: `NSOpenPanel` |
| `trash/` | `freedesktop.rs`: the Trash specification by hand | `macos.rs`: `NSFileManager`, and `freedesktop.rs` for the tests |
| `openers/` | `linux.rs`: desktop entries and `mimeapps.list` | `macos.rs`: Launch Services through `NSWorkspace` |
| `monitor/` | `wayland.rs`: the color-management protocol | `macos.rs`: `NSScreen` |
| `ui/fonts/` | `fontconfig.rs` | `macos.rs`: `NSFont` |

Everything else is one file with, at most, a `cfg!` inside it: `no_replace.rs`
(`renameat2` or `renamex_np`), `thumbnailer.rs::lower_priority`, `xdg.rs`'s
fallbacks, `thumbnail::Dirs::detect`, `input::logical_key`, and the keymap's
and gestures' `Default`. The application under `app/` does not know which
platform it is on, except for the one line in `main.rs` that answers
`--serve-clipboard` on Linux.

The Cocoa bindings are the `objc2` family, which wgpu already links for Metal,
each framework's classes behind a feature so only those named in `Cargo.toml`
are built. They are a target dependency of macOS alone; the Wayland crates and
fontconfig are Linux's alone.

## The main thread

AppKit puts a panel up and describes a screen on the main thread only, and
`objc2` makes that a type: `NSOpenPanel::openPanel` and `NSScreen::screens`
take a `MainThreadMarker`. Both are asked from the event loop, which is the
main thread, so neither needs a thread of its own:

- **The file dialog** runs modally in `portal::choose_on_thread` and hands its
  answer to the same `Deliver` the Linux dialog's thread does. The event loop
  is inside the modal session meanwhile, as every Mac program's is.
- **The monitors** are read afresh each time the table is asked, which
  `App::sync_monitor` does on every turn of the loop. `Monitors::live` is what
  tells the shared table to call `macos::refresh` first; a stub for the tests
  is not live, so what a test sets stays set.

Everything else here is safe off the main thread and is called from where the
Linux half is: the pasteboard from the loader and the clipboard watch,
`NSWorkspace` and `NSFileManager` from the event loop. Each call off the main
thread is wrapped in an autorelease pool, since Cocoa may autorelease inside
it and a thread of this program's has no pool of its own.

## What differs, and why

**The clipboard needs no process to hold it.** The pasteboard server keeps
what was put there after the process that put it has gone, so `copy` is one
call. A pasteboard names its types by uniform type identifier, so an `Offer`
carries one and `receive` asks for it back. `public.tiff` is among the types
taken, because that is what a screenshot copied to the clipboard is. The
pasteboard hands a picture over whole, so a paste is held in memory before it
is written, unlike Linux's pipe; the same ceiling is checked.

**The trash is Finder's.** `trashItemAtURL` moves a file into the Trash of the
volume it is on and says where it went, which is all an undo needs: a rename
back that refuses to replace, through `rename_no_replace`. Finder's own record
of where a file came from is private, so Put Back does not know about a file
restored here, and a file restored in Finder is `Refused::Gone` here. A
process started from a terminal may be refused a look inside `~/.Trash`
without Full Disk Access; that is reported as a failure naming Put Back, never
as the file being gone. The tests' trash is a directory laid out by the
freedesktop specification on both platforms, so that the application's tests
of deleting and undoing run unchanged and no test touches the user's Trash.

**Monitors are keyed by display id.** The table is keyed by `monitor::key_of`,
which is the output name on Wayland and the Core Graphics display id on a Mac,
since two identical displays share a name there. A display is in HDR mode
where its potential headroom is above one; asking for extended range never
switches a Mac display's mode, so following the monitor costs nothing here.
The current headroom ramps up smoothly once the window shows extended-range
content and eases as the light changes, so it is read in eighths of a stop:
read exactly it would be new on every turn, and every new value is a frame and
a fresh lift of a gain-mapped picture. wgpu's Metal surface offers
`Rgba16Float` in extended linear sRGB and sets the layer's extended-range flag
itself, so `render/output.rs` chooses the surface as on Linux; see
[color](color.md).

**The faces are San Francisco.** The system font is one variable file, so the
bold is the same file at another weight. `Face` carries the weight to set on
the file's `wght` axis, which egui applies through `FontTweak::coords`, and a
static face is handed over as it is. The file is found through the font
descriptor's file URL, and a face in a collection by its PostScript name.

**The thumbnail cache is this program's own**, under
`~/Library/Caches/com.dcervelli.gamut/thumbnails`, laid out as the freedesktop
cache is. A Mac has no cache shared between programs that anyone else writes
this way, and QuickLook's is private.

**The thumbnailer's thread goes into Darwin's background band**,
`setpriority(PRIO_DARWIN_THREAD, 0, PRIO_DARWIN_BG)`, which lowers its CPU
priority and throttles its reads together. `PRIO_PROCESS` is not used as on
Linux: on Darwin it is the whole process.

**LibRaw's pkg-config file names `stdc++`.** Homebrew's `libraw_r.pc` was
written for GNU's C++ library, which a Mac does not have, so `build.rs` prints
the link lines itself with `stdc++` read as `c++`.

## Files from Finder

A Mac starts a program for a file without naming the file: Launch Services
starts it with no arguments and then sends a `kAEOpenDocuments` Apple Event
with the files in it, and it sends the same event to a program already
running. winit 0.30 does not pass that event on, so `finder.rs` answers it
itself, with a handler on `NSAppleEventManager`, and hands the paths to the
loop as `UserEvent::Opened`, which `App::open_named` takes as it takes what
the file dialog chose. A program started this way has no command line, so it
opens on the empty window, sized by the first picture as any empty window
is.

The handler is installed on `NSApplicationWillFinishLaunchingNotification`,
observed from `main` before the loop runs. AppKit installs its own handler for
the event while it finishes launching and delivers the launching event just
after that notification, so this is the one moment a handler both survives
and hears the first files. An observer is used rather than the application's
delegate because the delegate is winit's.

## Keys and gestures

The key table is the same; `input::MAC_DEFAULTS` is the names whose chords
differ, bound over it by `Keymap::mac`, and `Gestures::mac` sets the three
slots that differ. Both are built on every platform, and their tests run
wherever the suite does; `Default` is the Mac's only on macOS. The reasons,
and the Option key's reading, are in [keys and gestures](keymap.md#on-a-mac).

## Not yet

- There is no application bundle, so `APP_ID` is only what `openers` leaves
  out of its own menu. The Dock's icon is set by the running program instead,
  from `packaging/`'s SVG, which AppKit reads itself (`window::show_icon`);
  Finder and Launchpad have none to show.
- The trackpad's smart zoom, a double tap, is not read.
- The menu bar is winit's default: the application menu with Hide and Quit.
  Its Quit reaches `App::exiting` through winit's `applicationWillTerminate:`,
  so the state is written as on any other quit.
