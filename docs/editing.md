# Deleting, renaming, removing, undo and exporting

What the program does to files on disk: a file is moved to the trash, or
renamed, and either is undone; or the picture as shown is exported to a
new file beside it. The keys are in [the user's
table](../user-docs/KEYS.md#renaming-and-deleting); this page is why the
code is shaped as it is.

## The trash is the desktop's

`src/trash.rs` follows the freedesktop.org Trash specification by hand: a
`files/` directory and an `info/` directory under `$XDG_DATA_HOME/Trash`,
each thrown-away file under a name unique within the trash and a
`<name>.trashinfo` beside it giving the original path, percent-encoded as a
URI's path is, and the deletion date in local time — `clock::local`, which
is why the tree already had a local clock and does not need a date library
for this. The point of following the specification rather than keeping a
trash of the program's own is that the file manager's Trash is where a
person looks for something they threw away, and it is under a retention
policy — emptied when the user empties it — that this program would
otherwise have to invent.

`Trash::put` writes the info file first, exclusively (`create_new`), which
is what makes the name unique against another program trashing a file of
the same name at the same moment; then renames the file under it through
`renameat2` with `RENAME_NOREPLACE`, so that a stale file in `files/` is
not overwritten either, and tries the next name — `photo.2.jpg`, the number
before the extension so that a file manager still reads the kind — if
either half finds the name taken. A file on another filesystem goes to the
`.Trash-<uid>` at the top of its own mount, found by walking up until the
device number changes, so that it is still a rename; only where that
directory cannot be made is it copied into the home trash, which is the
other thing the specification allows and the one that takes as long as the
file is large.

What `put` hands back is the `trash::Entry` it made — the held file, the
info file and the original path — and `trash::restore` takes exactly that.
An entry rather than a path because the trash is allowed to hold several
files that came from one path, and "the newest thing called `a.png`" is a
guess that the user's own next deletion, or another program's, breaks. A
restore refuses to replace, through the same `renameat2`, and reports why
it did not happen as `Refused::Taken` or `Refused::Gone` so that the
message at the foot of the window can say which.

## The list keeps a trashed file until something replaces it

Deleting the file on screen would be simplest as "take it off the list and
show the next", but everything the interface says about the picture — the
title, the tooltip on the name, what `kept.rs` puts settings away under —
is read from `Files::shown_path`, and for the milliseconds (or, with a
neighbor that will not decode, for good) between the deletion and the next
file arriving, that path has to still be the deleted file's. The one
exception is the last file on the list, which has no neighbor to wait for:
`Files::step_away` answers `None`, `App::leave_picture` takes the picture
down — keeping what it was left in under its path first, since undo will
want it back — and `Files::remove_shown` empties the list, leaving the
empty window described in [the interface](interface.md#the-empty-window-and-the-file-dialog).
Undo then `reinstate`s the file at the head of an empty list, where
nothing is on screen to be moved along by it, and asks for it. Otherwise
`App::delete_shown` trashes the file, `condemn`s it on the list, points the
watch at it so the bar strikes the name through at once rather than half a second on,
and asks for the neighbor as a step — forward, or back from the end of the
list, since the walk the user was making is the one to continue. The
condemned file leaves the list in `Files::shown`, the moment a reply for
another file reaches the screen, with the shown index and any read in
flight moved down with it; a reply for the condemned file itself, a reload
that was already on its way, is dropped there rather than shown under
another file's index. A second `Delete` while the neighbor is still being
read is refused by `is_idle`, which is what makes holding the key delete
as fast as the files decode and never the file already on its way out.

A removal — `Backspace`, `App::remove_shown` — goes out through the same
door. `Files::hide` puts the path in a set the [file list](filmstrip.md)
page describes and then `condemn`s it, so that everything above holds:
the file stays on screen until its neighbor is up, the last file empties
the window, and undo before the neighbor arrives is a `reprieve`. What
differs is that nothing on disk moves and the watch is left alone, so the
bar does not call the file deleted. A file already leaving refuses both a
second removal and a deletion, each with a message that says which it is.

## One undo stack, and only for the disk

`src/app/edits.rs` holds a `Vec<Edit>`: `Trashed`, with the entry, the
path as the list spelled it, where it stood and whether a rebuild of the
list kept it; `Renamed`, with both spellings; and `Removed`, with the
path, where it stood and whether a rebuild kept it — a removal touches
nothing on disk, but it takes a file out of the walk, and that is as
much a mistake to reach for undo over. One stack and one key for all
three, because the moment a reader reaches for undo is the moment they
have just made a mistake, and that is no time to ask which kind. Only
what touched the disk or the list goes on it: the view and the display
change dozens of times a session and have their own ways back — `z`,
`kept.rs` — and if they were here too, undoing a deletion would first
walk back through twenty exposure steps.

Last in, first out, for the session, uncapped: culling a directory is `]`
`Delete` `]` `Delete`, and "not that one, the one before" is two presses.
Restoring out of order, or after the window has closed, is what the file
manager's Trash is for, and the message about an undo that could not be
done says so. Whichever file an undo acts on is the file on screen
afterwards — a restored file goes back into the list by `Files::reinstate`
and is asked for; a file renamed back is asked for if it had been left —
since the message about it is the only other sign anything happened.

## The rename dialog is a modal, and its state is the application's

`src/ui/rename.rs` is the one `egui::Modal` in the tree. The menus and the
chooser are popups: things the picture can be looked at around, whose open
state lives in egui's memory so that `Esc` and a click outside close them
without the application knowing. A rename is a question, and nothing else
in the window should answer until it has; the modal dims everything behind
it and takes the pointer. Its state — which file, what the field says, what
is wrong with it — is `App::renaming`, handed in as `rename::Input` on
every frame it is up and handed back as `Command::Name` and presses of
`Control::RenameTo` and `Control::CancelRename`, so that the modal's own
`should_close` (its `Esc`, the click on the backdrop) comes back as the
same cancel the button sends.

What is wrong with a name is `rename::judge`, a function of two strings
and a closure: the words are the interface's, and whether a file of that
name is already in the directory is the filesystem's answer, which
`App::set_rename_name` asks with a `symlink_metadata` and hands in. That
keeps the judgment testable against nothing but strings, and keeps the
order of what stops a rename in one place — unchanged first, since the
file's own name is nothing to do whatever else is true of it; then whether
it is a name at all, a slash meaning a move and the dialog not moving;
then the directory. The extension changing is a `Fine` with a word beside
it rather than a refusal, set in the caution color where a refusal is set
in the warning color, and the field's outline follows the same split. The
rename itself goes through `trash::rename_no_replace` all the same: what
the dialog said was true when it was typed, and a file can have arrived
since.

The stem is selected as the dialog opens — `rename::stem_chars`, through
the `TextEditState` egui hands back from `TextEdit::show` — since the part
before the extension is nearly always the part being changed, and a
selection that took the whole name would lose the extension to the first
key.

## Exporting writes what the screen shows

`Ctrl+E` writes the picture as it is on screen to a new file: turned,
cropped to the region where one is up, and through the window, exposure,
curve and false color. It is the copy to the clipboard written to disk
instead — the same `encode::displayed` walk, then `encode::png_for_file`
(the clipboard's PNG squeezed harder, since the file is kept) or
`encode::jpeg` at the quality the dialog's slider says — so what is
exported cannot drift from what is copied, nor either from the readout. A crop is not a state of its own: the region
already is one, with the handles and the pixel-precise keys it needs.

The dialog is `ui/export.rs`, drawn with the rename dialog's pieces and
holding its state in `App::exporting` for the same reason. What it says
about the file is
`export::warnings`, a function of `export::Facts` that `App::open_export` gathers
once as it opens and of the format chosen, so the words are tested against
nothing but values. It warns only of what the new file loses that the
screen does not show: that the export looks like the screen is the point of
it, and a JPG's loss is what its quality slider is for. None of the
warnings refuse, since each is a price the user may want to pay. Only the name refuses, by
`export::judge`, and a name taken is refused rather than confirmed: the new
file never replaces anything, the file on screen least of all.

The write is on a copying thread, through `Copying::spawn_aside` rather than
`spawn`: a copy to the clipboard supersedes the copy before it, since only
the last one asked for should end up on the clipboard, but an export is
never stale, and must not cancel a copy in flight either. It is written under a
temporary name in the target directory and moved into place by
`trash::rename_no_replace`, so a file that arrived under the name after the
dialog judged it is not written over and a write cut short leaves nothing
under the name. The thread reports `Done::Exported` with the path, and
`App::exported` takes the file into the list by `Files::adopt`, as a paste
is: it arrives as a new file with nothing kept, which is right, since what
was done to the picture is in its pixels now.

The size the picture is written at is three boxes — a percentage, a width
and a height — that say one thing between them, since the aspect is
locked: `export::Resize` holds what each says, and `Resize::edit`, reached
by `Command::ExportSize` through `App::set_export_size`, works the size
out from whichever box was typed in and rewrites the other two. The box
typed in keeps its text as typed, so a `.` on its way to a decimal is not
taken away by a rewrite, and a percentage is written back to two decimals.
A side is a whole number of pixels from one to `export::SIDE_MAX` — 2¹⁵,
the largest texture the program shows — and a side worked out in
proportion is held to the same ceiling but never under a pixel; a box
that will not do is outlined, said under the row, and holds Export, while
the last size that would do is kept so that the warnings still speak of
something. The percentage is of the region where one is up, since that is
what is written. The write goes through `resample::resize` between the
`encode::displayed` walk and the encoder — see
[resampling.md](resampling.md) for what it does — and the picture is
enlarged bicubic whatever the screen's own filter; `Warning::Bicubic` says
so where the screen shows nearest, the one warning that is about what the
new file gains rather than loses.

One export at a time. The file each writes is taken into the list and
shown as it lands, so a second under way would land on top of the first
and the window would jump twice; and a large picture in a debug build
takes long enough for a second press to be tempting. `Copying` counts the
work aside it has started and not yet seen reported — `aside_pending`,
counted from the start to the report's arrival at a poll rather than from
the thread, so that the answer changes only when the loop looks — and
while it is pending the dialog opens as ever but Export and `Enter` are
dead, with a line saying why; `App::export_shown` refuses the press too,
so the button and the press cannot disagree.

An export is not an edit and is not on the undo stack. It changes nothing
that was there; the new file is deleted like any other.

A playing animation is stopped while the dialog is up and set playing again
when it goes, by `App::close_export`, which both Cancel and Export go
through. The frame written is read when Export is pressed, so without the
stop it would be whichever frame the clock had reached by then rather than
the one the dialog was opened on.
