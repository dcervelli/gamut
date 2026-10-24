# Commands

The configuration file can define commands: a line of the shell run on the
file on screen, with the window's state in its environment, which can ask
the window to show an image it made and say something, and whose output is
shown in the information panel's Output tab. How a user writes one, and
every variable and directive, are in
[`user-docs/SETTINGS.md`](../user-docs/SETTINGS.md#commands); this page is
why it is built as it is.

Three files hold it, split along the tree's layers. `src/commands.rs` is the
reading of the `[command.<name>]` tables, the result file's directives and the run
itself, as `src/exiftool.rs` is for exiftool: it knows nothing of the
application. `src/app/commands.rs` is the application's side: the set in
force, the one run in flight, the dialog's values, each file's last output,
the environment, and what a finished run does. `src/ui/commands.rs` is what
a frame draws — the menu's items, the dialog — and `src/ui/output.rs` the
Output tab.

## Read from the configuration file

A command is a TOML table, `[command.<name>]`, and its inputs an array of
tables under it — see [the configuration file](configuration.md) for how
the file is read. `Config::parse` reads the `command` table before the rest,
so that `keys.command.redact` and `gesture.image.middle.click =
"command.redact"` work wherever they sit in the file; the commands are in the
file's order, which is the menu's order and the index `Action::Command` and
`Control::RunCommand` carry. An input's kind and its range are checked by
`Kind::number`, `Kind::radio` and `Kind::slider` in `src/ui/commands.rs`,
which the old format's grammar is read through too when a file is rewritten.

The keymap is a table of `&'static` rows, so the names a file makes are
held beside it: `Keymap::add_extra` makes `command.<name>` a plain name, in
no context, bound to nothing. `Keymap::bind` takes chords across the table's
names and the extras alike, so a command's key is taken from whichever name
held it, and the other way round, as between any two names. `help_of`
answers with a `Cow`, and the help popup's `Row::does` is one, since an
extra's words are the file's.

## The environment, and why not the command line

Everything the window knows is handed over as `GAMUT_*` variables, never
spliced into the line. A file's metadata is anyone's to write; a camera
name substituted into a shell line would be a command. The variables are
set only where there is something to say, never empty, so `[ -n
"$GAMUT_LAT" ]` is the test a script makes. `GAMUT_` rather than `G_`,
which is GLib's own namespace (`G_DEBUG`, `G_MESSAGES_DEBUG`) and reaches
every GTK program the script might start. Free-form strings from a file's
metadata pass through `escape_controls`, since a NUL cannot be in the
environment and a control character has no business reaching a terminal the
script prints to.

The line runs as `/bin/sh -c <line> gamut`, so `$0` is `gamut` and a leading
`~/` is the shell's to expand; in the folder of the file on screen, so a
relative path in the line and in an `open` means the file's neighbors; with
stdin empty, so a script waiting for input ends rather than hangs; and in a
process group of its own, so that a stop reaches everything it started. On
a Mac a program started from the Dock has none of the user's shell startup,
so the path gets Homebrew's and MacPorts' directories appended where they
are missing.

### The file's own pixels

The size, the pointer and the region are held in the turned picture (see
[the interface](interface.md)), and the decoded picture is upright: the
decoders apply the EXIF orientation tag to the pixels. A script reads the
file, whose bytes lie in neither. So every position and size handed over is
undone twice: through the user's `Turn` with `Turn::stored`, then through
the tag with `orient::raw`, which is `orient::stored` given the upright
picture's size. A region goes through the same two by its two opposite
corner pixels (`Region::stored`, `Region::raw`), since each of the eight
orientations carries a rectangle to a rectangle. With that, `magick
"$GAMUT_FILE" -draw "rectangle …"` marks the place the user marked, with no
`-auto-orient`, and the file written keeps its tag. `GAMUT_ORIENTATION` and
`GAMUT_TURN` are both given, rather than one code folding them, so that a
script that does orient the pixels first can map back; folded, the number
would describe neither the file nor the screen.

The pointer and the region are left out while one of the picture's maps is
shown in its place, whose pixels are its own, and a command needing either
is dead then. The georeference reads the decoded grid — a georeferenced file
carries no orientation tag — so its variables are worked out from the
picture as decoded, the corners through `Georeference::model_at`, the
fractional twin of `latitude_longitude_at`, and `GAMUT_GEO_TRANSFORM` in
GDAL's order from the raster's corner.

The field of view shares the full frame's diagonal out by the picture's
aspect, as `image::attitude` does to work out a focal length in pixels: the
35 mm-equivalent focal length is defined against that diagonal, not against
a 36 mm width, so a 4:3 phone picture's horizontal angle comes out right.

## The result file, and why not the output

What a command asks of the window comes back in a file the window names,
`GAMUT_RESULT`, read with the configuration's own lexer after the run:
`open`, `message` and `error` lines. The output is never read for them. A
script very often prints what it read from a file — exiftool's tags, the
text in a picture, a caption — and if a line of output could be a
directive, the file's author could make the window open what they liked.
This is how GitHub retired `::set-output` for `$GITHUB_OUTPUT`. An unknown
line in the result file is an error the window says, since that file is the
script's deliberate answer; the output is free text, shown verbatim.

`open` takes a path only, and only one that exists and that `decode::probe`
reads, so that the list never takes in a file it cannot show. A URL would
hand the script's words to `xdg-open`, which starts whatever claims the
scheme; a script that wants a browser runs one itself. A run that replaced
its file wrote into a folder taken away as the run ends, so its `open`s are
read against it first: `GAMUT_OUTPUT` is the replaced file by then, and is
opened as that, and any other path inside the folder is refused, since it
would join the list and be gone a moment later. The paths are compared as
written, made absolute with `.` and `..` read away, since the output may
already have been moved. What is refused goes in the Output tab as well as
in a message, since the message about a replacement is said over it. A path already on the list is
gone to with `Files::go_to`, found by its absolute form since the list may
hold it relative; any other joins it beside the file on screen with
`Files::adopt`, as an export does. A script that writes beside the picture
or over it needs no `open` at all: the file on screen and its folder are
watched (see [live reload](live-reload.md)).

## One run at a time, delivered and stopped

Every command is dead while one runs. Two at once would race for the same
file and the same toast, and nothing about a viewer wants a queue.
`Conditions::running` makes `RunCommand` dead in `tooltip::disabled`, which
`App::refuses` reads for the key and the press, and the menu item's
`enabled` for its drawing, so the three cannot disagree. What each command
`needs` rides on the control itself, in `Slot`, since `Conditions` is a
`Copy` set of flags and a command's needs are the file's to choose.

The run is on a thread of its own that waits for the child, reading each
stream on a helper thread of its own so that neither pipe can fill and
stall the other, and keeping the first `CAPTURE_CAP` bytes of each. Its
answer reaches the loop as `UserEvent::Ran`, the way exiftool's does, rather
than by polling as `Copying` does: a command's result is what the user is
waiting to see, and a quarter second of the poll's cadence would be felt.

The message that says a command is running is a `Toast::stoppable`: up for
as long as the run, raised after `files::SLOW_READ` as a slow read is said,
with a cross that presses `Control::StopCommand` rather than dismissing.
That sends `SIGTERM` to the run's process group, through the `Stopper` the
thread holds the group in while the child lives. Without it a script stuck
on the network would leave every command dead until the window closed. The
window closing waits for nothing: a long job outlives it, and gets `SIGPIPE`
the next time it prints. A run that replaces its file is the exception,
stopped by `Commands::abandon` in `App::exiting` with its output directory
taken away, since nothing would be left to swap its output in.

## Replacing the file

A command with `output = "replace"` changes the file it ran on, and the
change is an undo step like a deletion or a rename. The script never names
the file to replace and never writes over it: it writes to `GAMUT_OUTPUT`,
and the window makes the swap. A script that edited the file in place could
not be undone, and one that named its own target could replace anything.

`GAMUT_OUTPUT` is the file's own name inside a directory made fresh for the
run beside the file, `.gamut-<pid>-<run>` from `commands::make_output_dir`.
Beside it, so that it is on the file's filesystem and the swap is a
rename; a directory, so that the output keeps the file's name and with it
the extension a tool such as ImageMagick picks the format by; hidden, so
that `listing::contents`, which skips hidden directories, never lists the
output as an image that arrived. `App::prepare_replacing` makes it on the
loop before the run starts, refusing a link, whose replacement would be a
file where the link was, and records the file's `watch::Signature`. The
directory is removed whole when the run is over, however it ended.

The swap is `App::replace_file` in `src/app/edits.rs`, called from
`App::command_finished` before the result file's directives, so that an
`open` of the file finds the new one. It is made only after an exit of 0,
and only where the output is a regular file that is not empty, the file's
signature is the one recorded at launch — a file changed under the run
would have its change lost — and `decode::probe` reads the output's header,
since a file nothing can show would leave the window with a file it could
not show. The output takes the original's permissions, then
`Trash::replace` makes the swap; see
[editing](editing.md#a-replacement-keeps-the-original-without-moving-it).
The file replaced is the one the run was launched on, held in
`Replacing::file`, not whatever is on screen when the run ends; it is read
again only if it is still the file on screen.

The window's message about the replacement is raised last, over any
`message` the script wrote: the toast has one slot, and that the original
is in the trash and how to undo is the thing a reader must not miss.

## The dialog

A command with inputs, or with `confirm`, opens a modal first, drawn with
the rename dialog's pieces: `rename::modal`, `rename::boxed_field`,
`rename::line` and `rename::ok_cancel`, so it reads as one of the window's
dialogs. Each input's value comes back as `Command::DialogInput` and is held
by the application, which judges a number as it is typed. The values a
dialog was let go with are remembered for the session by the command's name,
and used again only where each still fits its input — the file may have
been edited since — and never written to the state file, since what is
typed into a dialog may be a secret. Reading the configuration again takes
the dialog down, since the command it asked for may have changed under it.

A dialog can hold no text field — checkboxes alone, or only the question
`confirm` asks — and then egui does not want the keyboard and every key
would reach the key table too, `Esc` included. So `App::handle_key` does
nothing while the dialog is up; egui reads `Enter` and `Esc` for it.

## The output

Each run's output is kept for the file it ran on, the last `RUNS_KEPT`
files' worth, as the Tags tab keeps exiftool's answers. The Output tab is
on the info panel only while the configuration defines a command or the file
has a run kept, so a user without commands never meets it. It comes up,
without taking down what is open, when a run printed anything or failed
with errors — `App::reveal_info`, the half of `show_info_on` that only
opens. The rows are drawn only where they are in view, one line each, cut to
the panel; a click copies the whole line.

The output is also written to the run's directory under the cache,
`runs/<pid>-<n>`, made fresh with mode 0700, so that **Open log** can hand
it to the editor the configuration file opens in; the last `RUNS_KEPT`
directories are kept. The picture as shown, for a command that asks for it,
is written there too and removed when the run ends: under the cache rather
than `XDG_RUNTIME_DIR`, whose small quota a large PNG can exceed, and
readable by its owner alone.
