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

## Variants

A variant is read into the set as a `Command` of its own, after its base:
`read_variant` clones the base, gives it the variant's name and label, its
`output` and `confirm` where it says, and holds each input it sets in
`Input::set`, checked by `Kind::value_of` against the input's kind. Making
it a whole command rather than a reference to one is what keeps everything
downstream unchanged: `Action::Command` and `Control::RunCommand` carry its
index, `Keymap::add_extra` gives it a name to bind, the help lists it, and
`App::press_command` runs it, none of them knowing variants exist. Only
`run`, `needs`, `shown`, `name` and the inputs are not a variant's to
change: what it needs is what makes it dead, and a variant that needed
something its base did not would be a second command written in the
first's table.

`Command::base` is what remains of the family, read in three places.
`Command::family` keys what a dialog is remembered by, so a variant's
dialog opens with what its base's was left with; `Commands::remember`
writes back only the inputs that dialog asked for, so that a variant's
settings never become the base's defaults. `App::command_items` gives every
member of a family the base's label as its `Item::submenu`, and the file
menu and the Mac's list hang each run of items under one title from one
submenu. `commands::offered_name` names a variant's new file by its family,
since the variant's name is a key's rather than a word for the file.

A value the dialog never asks for — a `hidden` input, or one a variant
sets — is still in the environment: `input_environment` takes the dialog's
value for each input it asked and `Input::given` for the rest. So a script
reads every input the same way however it was run, and what a variant
means is in the values it sets rather than in its name, which a script
never sees.

`Command::asks` is what decides whether a press opens the dialog: an input
left to ask, `output = "choose"`, `confirm`, or a plain command's
`output = "new"`, whose name is asked. A variant that makes a file and has
nothing else to ask runs at once under the name `offered_name` gives,
which `prepare_making` counts up past a name taken meanwhile as it does
for any other: a variant is there to be one key, and a name the user would
only ever accept is not a question.

## Programs

A command's `programs` are looked for by `commands::locate` — the run's own
`PATH`, widened on a Mac as the run's is, then `exiftool::EXTRA_DIRS` — and
handed to it as `GAMUT_PROGRAM_<NAME>`. So a script never looks for one,
nor knows where a Mac's packages put it, and a script that runs
`"$GAMUT_PROGRAM_MAGICK"` runs exactly what the menu was judged by. `Commands::locate` keeps what it found for
`LOOK_AGAIN`, since it is asked on every frame, for every command, by
`App::command_blocked`; a program installed while the window is open is
noticed within it. `exiftool`, with no `path` of its own, is looked for by
the Tags tab's setting, so that one setting places it for both.

A missing program, and a replacement of a kind of file the command cannot
write, are reasons of the command's own rather than conditions every
command shares, and they carry the program's label, which
`ui::tooltip::disabled`'s `&'static str` cannot. So `App::command_blocked`
says them: `App::refuses` asks it beside `disabled`, `App::command_items`
draws the item dead by it, the `Namer` carries each command's reason for
the tooltip, and a key that runs a blocked command says the reason as a
message, since a key has no item to wear it.

`writes` is a program's, not a command's, since what ImageMagick writes is
the same for every command that runs it. A command writes the intersection
of its programs' lists (`commands::resolve`), worked out once the
configuration's programs and the built-ins' are both known. It is enforced
before the run rather than by the script: `command_blocked` for a command
that replaces, `Naming::replace`'s reason for the box that chooses to,
`App::unwritten` for a name typed, and `commands::offered_name`, which
offers the first extension the programs write where they do not write the
file's own — so that `redact-copy` on a raw runs at once as a PNG rather
than failing. `decode::other_kind` stays behind all of it for a script that
writes something else anyway.

## Built-in commands

`src/builtins.toml` is written as a configuration file is, and read by the
same `commands::parse` and `parse_programs`, so a built-in is nothing a
user's command could not be, and `--print-config` can print the file for
copying (`commands::builtins_template`). Its scripts are POSIX `sh`, one for
both platforms, asking `uname` where they differ, and held in the TOML
rather than as files beside the binary, since a script that has to be found
on disk is one more thing to install in the right place on every platform;
`sh -c` runs a script of many lines as it runs one.

`Config::default` holds the built-ins, so a configuration file that says
nothing has them, and `commands::with_builtins` lays a file's own over them:
its commands first, in its order, then each built-in family none of its own
replaces by name or turns off with `command.<name> = false`. A built-in
variant whose name one of its own already holds is left out alone, so that
a key bound to that name runs the user's. Keys are made after the merge,
`Config::adopt_commands` clearing the keymap's extras first, since the
indices the extras carry are the merged set's.

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
`open`, `message`, `error` and `step` lines. The output is never read for them. A
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

`step` goes through `App::step`, as `]` and `[` do, and only while the
file last asked for is still the one the run was given: a step taken while
the run went on, or a file the run opened or made, is what the user is
looking at now, and a run that ended later is not to move them off it. A
replacement leaves the same file asked for, read again, so a command that
replaces and steps on does both.

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

`GAMUT_OUTPUT` is `output.<ext>`, the file's own extension, inside a
directory made fresh for the run beside the file, `.gamut-<pid>-<run>` from
`commands::make_output_dir`. Beside it, so that it is on the file's
filesystem and the swap is a rename; a directory, so that the name is the
window's own and nothing else's; hidden, so that `listing::contents`, which
skips hidden directories, never lists the output as an image that arrived.
The name is `commands::output_name`'s rather than the file's, so that no
name a person chose reaches the script's arguments: ImageMagick reads a `%`
in an output name as a frame number and a `[` as a selection, and a file
called `100%.jpg` would be written somewhere else. The extension is kept,
since it is what such a tool picks the format by. `App::prepare_replacing` makes it on the
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
not show. `decode::other_kind` then holds the output to the original's
kind, by what the leading bytes of each say: the name does not change, so
a PNG written for a raw would read in the window and be wrong to every
other program that trusted its extension. A camera's raws count as one
kind, since `raw_format` tells them apart by the maker in the header,
which would refuse an NRW put back where it was read from. The output takes the original's permissions, then
`Trash::replace` makes the swap; see
[editing](editing.md#a-replacement-keeps-the-original-without-moving-it).
The file replaced is the one the run was launched on, held in
`Replacing::file`, not whatever is on screen when the run ends; it is read
again only if it is still the file on screen.

The window's message about the replacement is raised last, over any
`message` the script wrote: the toast has one slot, and that the original
is in the trash and how to undo is the thing a reader must not miss.

## A new file

`output = "new"` is the same arrangement for a file that was not there:
the script writes to `GAMUT_OUTPUT` and the window puts it in place, as
`App::make_file` in `src/app/edits.rs`. The window owns the destination for
the reasons it owns a replacement's — the script never picks a name, so it
can never write over anything, and the new file is an undo step
(`Edit::Created`, as an export and a paste are) — and so that a script
need not number its files against what the folder holds: the dialog judges
the name as it is typed, with `ui::export::judge` and the folder asked,
and the place is taken by `rename_no_replace`, counting up past a name
taken while the run went on rather than failing a run that did its work.

The folder is the file on screen's, or `pasted::directory()` with nothing
open, which is made where it is not there yet; the scratch directory goes
in it, for the rename. `App::naming` works out what the dialog opens with,
`commands::offered_name` the name: the configuration's `name` with
`{stem}` filled in, or the file's stem and the command's name with the
file's extension, counted up past the folder's names as the export dialog's
`default_name` is. A command that needs nothing offers its own name and no
extension: one that generates or fetches a picture does not know its
format until it has it.

So a name's extension is a request, not a fact. `GAMUT_OUTPUT` carries the
extension asked for, or none, and once the run is over
`decode::extension_for` names the file by what the leading bytes say it is
— the extension asked for where the format answers to it, the format's own
where it does not or none was asked. A `.png` holding a JPEG opens in fewer
places than the same file called `.jpg`, and refusing a minute's
generation over a suffix would be worse than correcting it and saying so.

`output = "choose"` is either, as the dialog's box says. The box is dead
where `prepare_replacing` would refuse — no trash, or a link — so that the
refusal is seen before the run rather than after it, and is remembered for
the session by the command's family, as the inputs are. The name typed is
not remembered: it names one file.

## The dialog

A command with an input to ask, or with `confirm`, opens a modal first, drawn with
the rename dialog's pieces: `rename::modal`, `rename::boxed_field`,
`rename::line` and `rename::ok_cancel`, so it reads as one of the window's
dialogs. Each input's value comes back as `Command::DialogInput` and is held
by the application, which judges a number as it is typed; each row carries
its input's place among the command's, since the rows leave out what is not
asked. The values a dialog was let go with are remembered for the session
by the command's family, and used again only where each still fits its
input — the file may have
been edited since — and never written to the state file, since what is
typed into a dialog may be a secret. Reading the configuration again takes
the dialog down, since the command it asked for may have changed under it.

A string with `lines` above 1 is `rename::boxed_field` given those lines: a
multiline `TextEdit` in a scroll area, the box as tall as its lines. The
dialog takes `Enter` before any field can, so `keys` in
`src/ui/commands.rs` leaves a plain `Enter` alone while such a box has the
keyboard, asking egui's memory for the focus by the box's id, and takes
`Modifiers::COMMAND` with `Enter` everywhere — `Ctrl` here, `⌘` on a Mac —
so that a prompt of several lines still has a key that runs it. A value
remembered with a line break does not fit a box of one line, should the
file have changed it to one.

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
