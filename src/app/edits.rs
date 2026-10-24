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
//! A deletion is a move to the desktop's own trash (see `trash/`), so
//! that the file shows up beside everything else thrown away, restorable
//! from there whether or not this window is still open. A rename is a
//! rename, refused rather than replacing anything, and undone by the same
//! rename the other way. A removal touches nothing on disk: the file is
//! taken off the list for the session, and undo puts it back where it
//! stood. A replacement — a command's output put in the place of the file
//! it ran on — keeps the original in the trash, and undo puts it back over
//! the edited file, which goes to the trash in its turn: neither undoing
//! nor doing loses a file. A file the window made — an export, a paste, a
//! command's new file — is undone by moving it to the trash, from where it
//! can still be taken back.

use std::fs;
use std::path::{Path, PathBuf};

use super::App;
use super::input::Effect;
use crate::no_replace::rename_no_replace;
use crate::trash::{self, Entry, Refused, Unreplaced};
use crate::ui::rename::{self, TAKEN, Verdict};
use crate::ui::toast::Level;
use crate::watch::{Signature, Watch};

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
    /// A file a command's output took the place of: the entry the original
    /// is in in the trash, and the path both are called, as the list
    /// spells it.
    Replaced { entry: Entry, path: PathBuf },
    /// A file the window made — an export, a paste, a command's new file —
    /// by the path it joined the list as.
    Created { path: PathBuf },
}

/// A run of a command that replaces the file it ran on, as the window holds
/// it until the run is over: the file, where its output is to be written,
/// and the file as it was when the run began, which it has to still be for
/// the output to take its place.
#[derive(Debug)]
pub(super) struct Replacing {
    /// The file, as the list spells it.
    pub(super) file: PathBuf,
    /// The directory made for the output, taken away whole once the run
    /// is over, however it went.
    pub(super) dir: PathBuf,
    /// What the command is told to write: `GAMUT_OUTPUT`.
    pub(super) output: PathBuf,
    pub(super) was: Signature,
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

/// What the messages that say how to undo say about it: the chords bound to
/// undo, as the key table spells them, so that a message cannot name a key
/// that does something else — and nothing where none is bound.
pub(super) fn to_undo(keys: &super::keymap::Keymap) -> String {
    match keys.spelled("files.undo") {
        key if key.is_empty() => String::new(),
        key => format!(" {key} to undo."),
    }
}

/// What the window says where there is no trash to keep a file in.
pub(super) const NO_TRASH: &str = "No trash to move the file to: HOME is unset.";

/// The last part of `path`, for a message about the file.
pub(super) fn name_of(path: &Path) -> String {
    crate::escape_controls(&crate::basename(path))
}

impl App {
    /// The file on screen, where a change to it on disk or on the list can
    /// be made now: nothing is being read, there is one, and it has not
    /// already been taken off the list or moved to the trash — each of
    /// which is said in place of doing it again.
    fn editable(&mut self) -> Option<PathBuf> {
        if !self.files.is_idle() {
            return None;
        }
        let path = self.files.shown_path().map(Path::to_path_buf)?;
        if self.files.is_hidden(&path) {
            self.toast("Already taken off the list.", Level::Warning);
            return None;
        }
        if self.files.is_condemned(&path) {
            self.toast("Already in the trash.", Level::Warning);
            return None;
        }
        Some(path)
    }

    /// Steps away from the file on screen, which is leaving the list: to
    /// a neighbor, where there is one — a file `condemned` to the trash
    /// stays on the list until the neighbor arrives, and is watched for
    /// its return meanwhile — and otherwise into the empty window, the
    /// file taken off at once.
    fn step_off(&mut self, condemned: Option<&Path>) {
        match self.files.step_away() {
            Some(request) => {
                if let Some(path) = condemned {
                    self.files.condemn();
                    // The bar says the file has gone at once, rather than half
                    // a second on when the watch would notice.
                    self.watch = Watch::new(path);
                }
                let _ = self.send(request);
            }
            None => {
                self.leave_picture();
                self.files.remove_shown();
            }
        }
    }

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
        let Some(listed) = self.editable() else {
            return;
        };
        let Some(trash) = &self.trash else {
            self.toast(NO_TRASH, Level::Error);
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
        self.step_off(Some(&listed));
        self.toast(
            format!("Trashed {}.{}", name_of(&listed), to_undo(&self.keys)),
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
        let Some(path) = self.editable() else {
            return;
        };
        self.edits.push(Edit::Removed {
            index: self.files.index(),
            adopted: self.files.is_adopted(&path),
            path: path.clone(),
        });
        self.files.hide();
        self.step_off(None);
        self.toast(
            format!(
                "Took {} off the list.{}",
                name_of(&path),
                to_undo(&self.keys)
            ),
            Level::Message,
        );
    }

    /// A run that was to replace `replacing.file` has exited cleanly: its
    /// output put in the file's place, the original kept in the trash, and
    /// the file read again where it is on screen. Refused, the file left as
    /// it was, where the output is empty or will not read, or where the
    /// file changed or went while the command ran. Says whether the file
    /// was replaced: a refusal is said here, and a replacement by
    /// [`App::said_replaced`], once the caller has done what the run's
    /// result file asked.
    pub(super) fn replace_file(&mut self, label: &str, replacing: &Replacing) -> bool {
        let Replacing {
            file, output, was, ..
        } = replacing;
        let name = name_of(file);
        let refused = match fs::symlink_metadata(output) {
            Ok(written) if written.is_file() && written.len() > 0 => None,
            _ => Some(format!("{label} wrote nothing, so {name} was left alone.")),
        }
        .or_else(|| match Signature::of(file) {
            None => Some(format!("{name} was moved or deleted while {label} ran.")),
            Some(now) if now != *was => Some(format!(
                "{name} changed while {label} ran, so it was left alone."
            )),
            Some(_) => None,
        })
        .or_else(|| {
            crate::image::decode::probe(output).is_err().then(|| {
                format!("{label} wrote a file that can't be read, so {name} was left alone.")
            })
        });
        if let Some(refused) = refused {
            self.toast(refused, Level::Error);
            return false;
        }
        let Some(trash) = &self.trash else {
            self.toast(NO_TRASH, Level::Error);
            return false;
        };
        // The new file is read and written by whoever could the old one.
        let replaced = fs::metadata(file)
            .and_then(|old| fs::set_permissions(output, old.permissions()))
            .map_err(|error| {
                Unreplaced::Untouched(anyhow::Error::from(error).context(format!(
                    "giving the new {} the permissions of the old",
                    crate::shown_path(file)
                )))
            })
            .and_then(|()| trash.replace(file, output));
        let entry = match replaced {
            Ok(entry) => entry,
            Err(Unreplaced::Untouched(error)) => {
                super::input::report(&error);
                self.toast(
                    format!(
                        "Could not replace {name}: {}. The original is untouched.",
                        crate::escape_controls(&format!("{error:#}"))
                    ),
                    Level::Error,
                );
                return false;
            }
            Err(Unreplaced::InTrash(error)) => {
                super::input::report(&error);
                self.toast(
                    format!("Could not replace {name}. The original is in the Trash."),
                    Level::Error,
                );
                return false;
            }
        };
        self.edits.push(Edit::Replaced {
            entry,
            path: file.clone(),
        });
        self.file_replaced(file);
        true
    }

    /// Says that `label` replaced `file`, and how to take it back.
    pub(super) fn said_replaced(&mut self, label: &str, file: &Path) {
        self.toast(
            format!(
                "{label} changed {}. The original is in the trash.{}",
                name_of(file),
                to_undo(&self.keys)
            ),
            Level::Message,
        );
    }

    /// The file at `path` is another file under the same name: what was
    /// made of it is made again — its thumbnail, its facts, its place in an
    /// order that reads them — and it is read again where it is on screen.
    /// The read's answer brings the watch on it up to date, so the change is
    /// not read a second time when the watch next looks.
    fn file_replaced(&mut self, path: &Path) {
        self.rewritten(vec![path.to_path_buf()]);
        if self.files.shown_path() == Some(path)
            && self.files.is_idle()
            && let Some(request) = self.files.reload()
        {
            let _ = self.send(request);
        }
    }

    /// Opens the rename dialog on the file on screen, its name in the field.
    pub(super) fn open_rename(&mut self) {
        // One thing at a time: a menu still open under a dialog would be a
        // second thing on screen asking for a press, and so would the other
        // dialog.
        if self.exporting.is_some() || self.commands.asking.is_some() {
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
                .is_some_and(|dir| super::exporting::taken(dir, typed))
        });
        renaming.name = name;
    }

    /// OK: renames the file the dialog was opened on to what the field
    /// says, if the name will do, and puts the dialog away.
    ///
    /// The directory is asked again by the rename itself, which refuses to
    /// replace: what the dialog said about the name was true when it was
    /// typed, and a file can have arrived since.
    /// Renames `from` to `to` on disk, refusing to replace whatever is at
    /// `to`, and says how it went: `taken` is what to say where something
    /// is, and anything else that goes wrong is said with `doing` for what
    /// was being done. Whether the file is called `to` now.
    fn rename_file(
        &mut self,
        from: &Path,
        to: &Path,
        doing: &str,
        taken: impl FnOnce() -> String,
    ) -> bool {
        match rename_no_replace(from, to) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.toast(taken(), Level::Warning);
                false
            }
            Err(error) => {
                let error = anyhow::Error::from(error).context(format!(
                    "{doing} {} to {}",
                    crate::shown_path(from),
                    name_of(to)
                ));
                super::input::report(&error);
                self.toast(super::input::briefly(&error), Level::Error);
                false
            }
        }
    }

    pub(super) fn rename_shown(&mut self) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        if !renaming.verdict.allows() {
            return;
        }
        let from = renaming.path;
        let to = from.with_file_name(&renaming.name);
        if !self.rename_file(&from, &to, "renaming", || TAKEN.to_string()) {
            return;
        }
        self.renamed(&from, &to);
        self.toast(
            format!("Renamed {}.{}", name_of(&from), to_undo(&self.keys)),
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
                let back = || {
                    format!(
                        "Could not rename {} back: a file called {} is there now.",
                        name_of(&to),
                        name_of(&from)
                    )
                };
                if !self.rename_file(&to, &from, "renaming back", back) {
                    return Effect::Redraw;
                }
                self.renamed(&to, &from);
                if let Some(at) = self.files.position(&from)
                    && at != self.files.index()
                {
                    let request = self.files.go_to(at);
                    let _ = self.send(request);
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
                    let _ = self.send(request);
                    self.list_changed();
                }
                self.toast(
                    format!("Put {} back on the list.", name_of(&path)),
                    Level::Message,
                );
            }
            Edit::Replaced { entry, path } => self.unreplace(&entry, &path),
            Edit::Created { path } => self.uncreate(&path),
        }
        Effect::Redraw
    }

    /// Notes that the window made the file at `path`, which has joined the
    /// list as that, for undo to take away again.
    pub(super) fn created(&mut self, path: PathBuf) {
        self.edits.push(Edit::Created { path });
    }

    /// Takes away a file the window made: to the trash, so that an undo
    /// pressed once too often costs nothing, and off the list — stepping
    /// away from it as a deletion does where it is on screen.
    fn uncreate(&mut self, path: &Path) {
        let name = name_of(path);
        if fs::symlink_metadata(path).is_err() {
            self.forget_made(path);
            self.toast(format!("{name} is no longer there."), Level::Warning);
            return;
        }
        let Some(trash) = &self.trash else {
            self.toast(NO_TRASH, Level::Error);
            return;
        };
        if let Err(error) = trash.put(path) {
            super::input::report(&error);
            self.toast(super::input::briefly(&error), Level::Error);
            return;
        }
        self.forget_made(path);
        self.toast(format!("Moved {name} to the trash."), Level::Message);
    }

    /// A file the window made has gone: off the list, stepped away from
    /// where it is on screen.
    fn forget_made(&mut self, path: &Path) {
        if self.files.shown_path() == Some(path) {
            self.step_off(Some(path));
        } else if self.files.position(path).is_some() {
            self.files.forget(path);
            self.list_changed();
        }
    }

    /// Puts the original of a replaced file back, the edited one going to
    /// the trash in its place, and shows it.
    fn unreplace(&mut self, entry: &Entry, path: &Path) {
        let name = name_of(path);
        let restored = match &self.trash {
            Some(trash) => trash.restore_over(entry),
            None => Err(Refused::Failed(anyhow::anyhow!(
                "no trash, HOME being unset"
            ))),
        };
        match restored {
            Ok(()) => {}
            Err(Refused::Gone) => {
                self.toast(
                    format!(
                        "The original {name} is no longer in the trash. The edited file was kept."
                    ),
                    Level::Warning,
                );
                return;
            }
            Err(refused) => {
                let error = anyhow::anyhow!("{refused}");
                super::input::report(&error);
                self.toast(
                    format!(
                        "Could not restore {name}: {}. The edited file was kept.",
                        crate::escape_controls(&refused.to_string())
                    ),
                    Level::Error,
                );
                return;
            }
        }
        match self.files.position(path) {
            Some(at) if at != self.files.index() => {
                self.rewritten(vec![path.to_path_buf()]);
                let request = self.files.go_to(at);
                let _ = self.send(request);
            }
            _ => self.file_replaced(path),
        }
        self.toast(
            format!("Restored the original {name}. The edited file is in the trash."),
            Level::Message,
        );
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
            let _ = self.send(request);
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
            current.file = super::file_facts(to, current.file.reader);
            if let Some(shown) = &self.shown {
                shown.window.set_title(&super::window::window_title(to));
            }
        }
    }
}
