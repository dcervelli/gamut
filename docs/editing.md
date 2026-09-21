# Deleting, renaming and undo

What the program does to a file on disk, which until now was nothing: a
file is moved to the trash, or renamed, and either is undone. The keys are
in [the user's table](../user-docs/KEYS.md#renaming-and-deleting); this
page is why the code is shaped as it is.

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
file arriving, that path has to still be the deleted file's. So
`App::delete_shown` trashes the file, `condemn`s it on the list, points the
watch at it so the bar says `DELETED` at once rather than half a second on,
and asks for the neighbor as a step — forward, or back from the end of the
list, since the walk the user was making is the one to continue. The
condemned file leaves the list in `Files::shown`, the moment a reply for
another file reaches the screen, with the shown index and any read in
flight moved down with it; a reply for the condemned file itself, a reload
that was already on its way, is dropped there rather than shown under
another file's index. A second `Delete` while the neighbor is still being
read is refused by `is_idle`, which is what makes holding the key delete
as fast as the files decode and never the file already on its way out.

## One undo stack, and only for the disk

`src/app/edits.rs` holds a `Vec<Edit>`: `Trashed`, with the entry, the
path as the list spelled it, where it stood and whether a rebuild of the
list kept it; and `Renamed`, with both spellings. One stack and one key for
both, because the moment a reader reaches for undo is the moment they have
just made a mistake, and that is no time to ask which kind. Only what
touched the disk goes on it: the view and the display change dozens of
times a session and have their own ways back — `z`, `kept.rs` — and if
they were here too, undoing a deletion would first walk back through
twenty exposure steps.

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
