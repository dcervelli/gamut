//! What is done to the file on disk — moved to the trash, renamed — or to
//! its place on the list, and the stack that undoes it.
//!
//! One stack and one key for all of them, since the moment a reader reaches
//! for undo is the moment they have just made a mistake, and that is no
//! time to ask which kind. What goes on it is only what touched the disk
//! or took a file off the list: the view and the display are changed
//! dozens of times a session and put back by hand or by `z`, and if they
//! were here too, undoing a deletion would first walk back through twenty
//! exposure steps. Last in, first out, for the session: culling a
//! directory is `]` `Delete` `]` `Delete`, and "not that one, the one
//! before" is two presses. Anything more — restoring out of order, or after
//! the window has closed — is what the file manager's Trash is for, and a
//! message about a restore that could not be done says so.
//!
//! A deletion is a move to the desktop's own trash (see `trash.rs`), so
//! that the file shows up beside everything else thrown away, restorable
//! from there whether or not this window is still open. A rename is a
//! rename, refused rather than replacing anything, and undone by the same
//! rename the other way. A removal touches nothing on disk: the file is
//! taken off the list for the session, and undo puts it back where it
//! stood.

use std::path::{Path, PathBuf};

use super::App;
use super::input::Effect;
use super::input::{Action, binding_for};
use crate::trash::{self, Entry, Refused};
use crate::ui::rename::{self, TAKEN, Verdict};
use crate::ui::toast::Level;
use crate::watch::Watch;

/// One thing done to a file on disk, or to its place on the list, and
/// enough to undo it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Edit {
    /// A file moved to the trash: the entry it is in there, the path it
    /// was on the list as, and where it stood in the list — to put it back
    /// there — and whether the list keeps it through a rebuild, as it did
    /// a paste.
    Trashed {
        entry: Entry,
        listed: PathBuf,
        index: usize,
        adopted: bool,
    },
    /// A file renamed, by the list's own spelling of each path.
    Renamed { from: PathBuf, to: PathBuf },
    /// A file taken off the list, untouched on disk: where it stood, to
    /// put it back there, and whether the list keeps it through a rebuild.
    Removed {
        path: PathBuf,
        index: usize,
        adopted: bool,
    },
}

/// The rename dialog while it is up: which file, what the field says, and
/// what is wrong with it.
pub(super) struct Renaming {
    /// The file being renamed, as the list spells it. Not "the file on
    /// screen": a read that lands while the dialog is up must not change
    /// which file OK renames.
    path: PathBuf,
    name: String,
    verdict: Verdict,
    /// Set as the dialog opens and taken by the first frame — see
    /// [`rename::Input::opened`].
    opened: bool,
}

/// What to press to undo, for the messages that say so: the key table's own
/// word, so that a message cannot name a key that does something else.
fn undo_key() -> String {
    binding_for(Action::Undo).map_or_else(String::new, |binding| binding.shown.to_string())
}

/// The last part of `path`, for a message about the file.
pub(super) fn name_of(path: &Path) -> String {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    crate::escape_controls(&name)
}

impl App {
    /// Moves the file on screen to the trash, and steps on to the next.
    ///
    /// The file stays on the list, and on screen, until its neighbor has
    /// arrived: the picture is still what is being looked at, and the list
    /// is what says which file it is. With no neighbor that will decode it
    /// stays for good, marked in the bar as a file deleted under us is, and
    /// undo puts it back where it stands. The last file on the list has no
    /// neighbor to wait for: it leaves the list and the screen at once, and
    /// the window shows nothing until something is opened — or undo puts
    /// the file back at the head of the list, where it is shown again.
    pub(super) fn delete_shown(&mut self) {
        // One at a time: a key held down deletes as fast as the neighbors
        // decode, and never the file already on its way out.
        if !self.files.is_idle() {
            return;
        }
        let Some(listed) = self.files.shown_path().map(Path::to_path_buf) else {
            return;
        };
        if self.files.is_hidden(&listed) {
            self.toast("Already taken off the list.", Level::Warning);
            return;
        }
        if self.files.is_condemned(&listed) {
            self.toast("Already in the trash.", Level::Warning);
            return;
        }
        let Some(trash) = &self.trash else {
            self.toast("No trash to move the file to: HOME is unset.", Level::Error);
            return;
        };
        let entry = match trash.put(&listed) {
            Ok(entry) => entry,
            Err(error) => {
                super::input::report(&error);
                self.toast(super::input::briefly(&error), Level::Error);
                return;
            }
        };
        self.edits.push(Edit::Trashed {
            entry,
            index: self.files.index(),
            adopted: self.files.is_adopted(&listed),
            listed: listed.clone(),
        });
        match self.files.step_away() {
            Some(request) => {
                self.files.condemn();
                // The bar says the file has gone at once, rather than half
                // a second on when the watch would notice.
                self.watch = Watch::new(&listed);
                self.send(request);
            }
            // The picture first, kept under its path while the list still
            // names it; then the list.
            None => {
                self.leave_picture();
                self.files.remove_shown();
            }
        }
        self.toast(
            format!("Trashed {}. {} to undo.", name_of(&listed), undo_key()),
            Level::Message,
        );
    }

    /// Takes the file on screen off the list, leaving it as it is on disk,
    /// and steps on to the next. It goes out the way a trashed file does —
    /// staying on screen until its neighbor has arrived, or leaving the
    /// screen empty when it was the last — and undo puts it back where it
    /// stood. The directory being read again does not bring it back: see
    /// `Files::hide`.
    pub(super) fn remove_shown(&mut self) {
        // One at a time, as a deletion is, and never the file already on
        // its way out.
        if !self.files.is_idle() {
            return;
        }
        let Some(path) = self.files.shown_path().map(Path::to_path_buf) else {
            return;
        };
        if self.files.is_hidden(&path) {
            self.toast("Already taken off the list.", Level::Warning);
            return;
        }
        if self.files.is_condemned(&path) {
            self.toast("Already in the trash.", Level::Warning);
            return;
        }
        self.edits.push(Edit::Removed {
            index: self.files.index(),
            adopted: self.files.is_adopted(&path),
            path: path.clone(),
        });
        self.files.hide();
        match self.files.step_away() {
            Some(request) => self.send(request),
            None => {
                self.leave_picture();
                self.files.remove_shown();
            }
        }
        self.toast(
            format!(
                "Took {} off the list. {} to undo.",
                name_of(&path),
                undo_key()
            ),
            Level::Message,
        );
    }

    /// Opens the rename dialog on the file on screen, its name in the field.
    pub(super) fn open_rename(&mut self) {
        // One thing at a time: a menu still open under a dialog would be a
        // second thing on screen asking for a press, and so would the other
        // dialog.
        if self.exporting.is_some() {
            return;
        }
        self.close_menus();
        let Some(path) = self.files.shown_path().map(Path::to_path_buf) else {
            return;
        };
        let name = name_of(&path);
        self.renaming = Some(Renaming {
            verdict: rename::judge(&name, &name, |_| false),
            path,
            name,
            opened: true,
        });
    }

    /// The dialog's field changed: what is wrong with the name now, if
    /// anything, is worked out here — the directory asked whether the name
    /// is taken — for the next frame to say.
    pub(super) fn set_rename_name(&mut self, name: String) {
        let Some(renaming) = &mut self.renaming else {
            return;
        };
        let current = name_of(&renaming.path);
        let dir = renaming.path.parent().map(Path::to_path_buf);
        renaming.verdict = rename::judge(&current, &name, |typed| {
            dir.as_deref()
                .is_some_and(|dir| std::fs::symlink_metadata(dir.join(typed)).is_ok())
        });
        renaming.name = name;
    }

    /// OK: renames the file the dialog was opened on to what the field
    /// says, if the name will do, and puts the dialog away.
    ///
    /// The directory is asked again by the rename itself, which refuses to
    /// replace: what the dialog said about the name was true when it was
    /// typed, and a file can have arrived since.
    pub(super) fn rename_shown(&mut self) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        if !renaming.verdict.allows() {
            return;
        }
        let from = renaming.path;
        let to = from.with_file_name(&renaming.name);
        match trash::rename_no_replace(&from, &to) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.toast(TAKEN, Level::Warning);
                return;
            }
            Err(error) => {
                let error = anyhow::Error::from(error).context(format!(
                    "renaming {} to {}",
                    crate::shown_path(&from),
                    name_of(&to)
                ));
                super::input::report(&error);
                self.toast(super::input::briefly(&error), Level::Error);
                return;
            }
        }
        self.renamed(&from, &to);
        self.toast(
            format!("Renamed {}. {} to undo.", name_of(&from), undo_key()),
            Level::Message,
        );
        self.edits.push(Edit::Renamed { from, to });
    }

    /// Cancel, `Esc`, or a click outside: the dialog goes, and nothing
    /// changes.
    pub(super) fn cancel_rename(&mut self) {
        self.renaming = None;
    }

    /// What the dialog is drawn from this frame, while it is up.
    pub(super) fn rename_input(&mut self) -> Option<rename::Input> {
        let renaming = self.renaming.as_mut()?;
        Some(rename::Input {
            current: name_of(&renaming.path),
            name: renaming.name.clone(),
            verdict: renaming.verdict.clone(),
            opened: std::mem::take(&mut renaming.opened),
        })
    }

    /// Undoes the last edit: the file put back from the trash, its old
    /// name put back on it, or its place on the list given back. Whichever
    /// it is, the file it acted on is the one on screen afterwards, since
    /// the message about it is the only other sign anything happened.
    pub(super) fn undo(&mut self) -> Effect {
        let Some(edit) = self.edits.pop() else {
            self.toast("Nothing to undo.", Level::Warning);
            return Effect::Redraw;
        };
        match edit {
            Edit::Trashed {
                entry,
                listed,
                index,
                adopted,
            } => self.untrash(&entry, listed, index, adopted),
            Edit::Renamed { from, to } => {
                match trash::rename_no_replace(&to, &from) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        self.toast(
                            format!(
                                "Could not rename {} back: a file called {} is there now.",
                                name_of(&to),
                                name_of(&from)
                            ),
                            Level::Warning,
                        );
                        return Effect::Redraw;
                    }
                    Err(error) => {
                        let error = anyhow::Error::from(error).context(format!(
                            "renaming {} back to {}",
                            crate::shown_path(&to),
                            name_of(&from)
                        ));
                        super::input::report(&error);
                        self.toast(super::input::briefly(&error), Level::Error);
                        return Effect::Redraw;
                    }
                }
                self.renamed(&to, &from);
                if let Some(at) = self.files.position(&from)
                    && at != self.files.index()
                {
                    let request = self.files.go_to(at);
                    self.send(request);
                }
                self.toast(
                    format!("Renamed {} back to {}.", name_of(&to), name_of(&from)),
                    Level::Message,
                );
            }
            Edit::Removed {
                path,
                index,
                adopted,
            } => {
                if self.files.is_condemned(&path) {
                    // Never left: it stays, and that is all.
                    self.files.reprieve();
                } else {
                    let request = self.files.reinstate(path.clone(), index, adopted);
                    self.send(request);
                    self.list_changed();
                }
                self.toast(
                    format!("Put {} back on the list.", name_of(&path)),
                    Level::Message,
                );
            }
        }
        Effect::Redraw
    }

    /// Puts a trashed file back: on disk, and on the list — where it still
    /// is, if its neighbor never arrived, or back where it was.
    fn untrash(&mut self, entry: &Entry, listed: PathBuf, index: usize, adopted: bool) {
        match trash::restore(entry) {
            Ok(()) => {}
            Err(Refused::Gone) => {
                self.toast(
                    format!("{} is no longer in the trash.", name_of(&listed)),
                    Level::Warning,
                );
                return;
            }
            Err(Refused::Taken) => {
                self.toast(
                    format!(
                        "Could not put {} back: something else is at {} now.",
                        name_of(&listed),
                        crate::shown_path(&entry.original)
                    ),
                    Level::Warning,
                );
                return;
            }
            Err(Refused::Failed(error)) => {
                super::input::report(&error);
                self.toast(super::input::briefly(&error), Level::Error);
                return;
            }
        }
        if self.files.is_condemned(&listed) {
            // Never left: the bar stops calling it deleted, and that is all.
            self.files.reprieve();
            self.watch = Watch::new(&listed);
        } else {
            let request = self.files.reinstate(listed.clone(), index, adopted);
            self.send(request);
            self.list_changed();
        }
        self.toast(format!("Put {} back.", name_of(&listed)), Level::Message);
    }

    /// The file at `from` is called `to` now, and everything that knew it by
    /// name follows: the list, what it was left in, the watch on it, the
    /// name in the bar and the title, where it is the file on screen.
    fn renamed(&mut self, from: &Path, to: &Path) {
        if let Some(at) = self.files.position(from) {
            self.files.rename(at, to.to_path_buf());
            self.list_changed();
        }
        self.kept.rename(from, to);
        self.visited.rename(from, to);
        if self.files.shown_path() == Some(to)
            && let Some(current) = self.current.as_mut()
        {
            self.watch = Watch::new(to);
            current.label = super::window::file_label(to);
            current.file = super::file_facts(to);
            if let Some(shown) = &self.shown {
                shown.window.set_title(&super::window::window_title(to));
            }
        }
    }
}
