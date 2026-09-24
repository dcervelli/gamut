//! Exporting the picture as it is shown, to a new file beside the one on screen:
//! the dialog's state while it is up, the write on a thread of its own, and
//! the new file taken into the list once it is on disk.
//!
//! Not an edit of the file on disk, as a rename or a deletion is: nothing is
//! written over and nothing is undone. The new file is named in the dialog,
//! refused where the name is taken, and written under a temporary name first
//! and moved into place by a rename that will not replace — so that a file
//! that arrived under the name after the dialog judged it is not written
//! over either, and a write cut short leaves nothing under the name.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use super::App;
use super::copying::Done;
use super::edits::name_of;
use super::input::{briefly, report};
use crate::image::Samples;
use crate::image::region::Region;
use crate::image::sequence::Sequence;
use crate::image::{encode, resample};
use crate::loader::Source;
use crate::timing;
use crate::trash;
use crate::ui::export::{self, Dimension, Facts, Format, Frames, Resize, Verdict};
use crate::ui::rename::TAKEN;
use crate::ui::toast::Level;

/// The export dialog while it is up.
pub(super) struct Exporting {
    /// The file on screen as the dialog opened, as the list spells it: the
    /// new file goes in its directory.
    source: PathBuf,
    name: String,
    format: Format,
    /// What a JPEG is written at, from `encode::JPEG_QUALITY_MIN` to 100.
    quality: u8,
    /// The size the picture is written at, as the three boxes hold it.
    resize: Resize,
    verdict: Verdict,
    facts: Facts,
    /// Whether an animation was playing as the dialog opened, and was
    /// stopped so that the frame exported is the frame the dialog was
    /// opened on: it plays again when the dialog goes.
    resume: bool,
    /// Set as the dialog opens and taken by the first frame — see
    /// `rename::Input::opened`.
    opened: bool,
}

impl Exporting {
    /// The directory the new file goes in.
    fn dir(&self) -> PathBuf {
        self.source
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf)
    }

    /// Judges the name in the field again, the directory asked whether it
    /// is taken.
    fn judge(&mut self) {
        let dir = self.dir();
        self.verdict = export::judge(&self.name, |typed| taken(&dir, typed));
    }
}

/// Whether `name` is already something in `dir`, a link that leads nowhere
/// included: the rename that puts the file in place would refuse it.
fn taken(dir: &Path, name: &str) -> bool {
    std::fs::symlink_metadata(dir.join(name)).is_ok()
}

impl App {
    /// Opens the export dialog on the picture on screen, with a name for the
    /// new file that is not taken and the format the file on screen is in,
    /// where it is one of the two.
    pub(super) fn open_export(&mut self) {
        // One dialog at a time, and no menu under it.
        if self.renaming.is_some() {
            return;
        }
        self.close_menus();
        let (Some(current), Some(path)) = (&self.current, self.files.shown_path()) else {
            return;
        };
        let image = &current.image;
        let source = path.to_path_buf();
        let name = name_of(&source);
        let region = self.marking.selection.region();
        let facts = Facts {
            deeper_than_8_bit: !matches!(image.samples, Samples::U8 { .. })
                || image.gain_map.is_some(),
            alpha: image.channels().alpha_index().is_some(),
            frames: match (&self.animation, current.sequence) {
                (Some(animation), _) => Frames::Animation {
                    frame: animation.on_screen(),
                },
                (None, Sequence::Pages { .. }) => Frames::Pages { page: current.page },
                (None, _) => Frames::Still,
            },
            metadata: !current.exif.sections.is_empty(),
            region: region.map(|region| [region.width, region.height]),
            source: export::format_of(&name),
            upscale: self.view.upscale(),
        };
        let format = facts.source.unwrap_or(Format::Png);
        let stem = Path::new(&name)
            .file_stem()
            .map_or_else(|| name.clone(), |stem| stem.to_string_lossy().into_owned());
        let mut exporting = Exporting {
            source,
            name: String::new(),
            format,
            quality: encode::JPEG_QUALITY,
            resize: Resize::new(facts.region.unwrap_or_else(|| current.pixels())),
            verdict: Verdict::Empty,
            facts,
            resume: false,
            opened: true,
        };
        let dir = exporting.dir();
        exporting.name = export::default_name(&stem, format, |name| taken(&dir, name));
        exporting.judge();
        let now = Instant::now();
        if let Some(animation) = self
            .animation
            .as_mut()
            .filter(|animation| animation.playing())
        {
            animation.toggle(now);
            exporting.resume = true;
        }
        self.exporting = Some(exporting);
    }

    /// Puts the dialog away, and sets an animation it stopped playing
    /// again.
    fn close_export(&mut self) {
        let Some(exporting) = self.exporting.take() else {
            return;
        };
        if exporting.resume
            && let Some(animation) = self
                .animation
                .as_mut()
                .filter(|animation| !animation.playing())
        {
            animation.toggle(Instant::now());
        }
    }

    /// The dialog's field changed. An extension that names a format makes
    /// it the format written.
    pub(super) fn set_export_name(&mut self, name: String) {
        let Some(exporting) = &mut self.exporting else {
            return;
        };
        if let Some(format) = export::format_of(&name) {
            exporting.format = format;
        }
        exporting.name = name;
        exporting.judge();
    }

    /// A format's button: the name's extension follows it.
    pub(super) fn set_export_format(&mut self, format: Format) {
        let Some(exporting) = &mut self.exporting else {
            return;
        };
        exporting.format = format;
        if !exporting.name.is_empty() {
            exporting.name = export::with_extension(&exporting.name, format);
        }
        exporting.judge();
    }

    /// The quality slider moved.
    pub(super) fn set_export_quality(&mut self, quality: u8) {
        if let Some(exporting) = &mut self.exporting {
            exporting.quality = quality.clamp(encode::JPEG_QUALITY_MIN, 100);
        }
    }

    /// One of the size boxes changed: the size follows it where it will
    /// do, and the other two boxes follow the size.
    pub(super) fn set_export_size(&mut self, dimension: Dimension, text: String) {
        if let Some(exporting) = &mut self.exporting {
            exporting.resize.edit(dimension, text);
        }
    }

    /// Export: puts the dialog away and writes the picture as shown, at
    /// the size asked for, on a thread of its own, if the name and the
    /// size will do. What it did comes back through [`App::poll_copies`],
    /// which hands a file written to [`App::exported`].
    pub(super) fn export_shown(&mut self) {
        let Some(exporting) = &self.exporting else {
            return;
        };
        if !exporting.verdict.allows() || !exporting.resize.allows() {
            return;
        }
        let (format, quality, size) = (exporting.format, exporting.quality, exporting.resize.size);
        let to = exporting.dir().join(&exporting.name);
        self.close_export();
        let Some(current) = &self.current else {
            return;
        };
        let image = Arc::clone(&current.image);
        let display = current.display.clone();
        let lift = current.lift.clone();
        let turn = current.turn;
        let region = self
            .marking
            .selection
            .region()
            .unwrap_or_else(|| Region::whole(current.pixels()));
        self.copying.spawn_aside(move |ticket| {
            let walked = Instant::now();
            let raster = encode::displayed(&image, &display, turn, region, lift.as_deref());
            timing::mapped_image(region.width, region.height, walked.elapsed());
            let raster = resample::resize(raster, size);
            let bytes = match format {
                Format::Png => encode::png_for_file(&raster),
                Format::Jpeg => encode::jpeg(&raster, quality),
            };
            let outcome = bytes.and_then(|bytes| write_new(&to, &bytes));
            ticket.report(match outcome {
                Ok(()) => Ok(Done::Exported(to)),
                Err(error) => {
                    report(&error);
                    Err(briefly(&error))
                }
            });
        });
    }

    /// Cancel, `Esc`, or a click outside: the dialog goes, and nothing is
    /// written.
    pub(super) fn cancel_export(&mut self) {
        self.close_export();
    }

    /// What the dialog is drawn from this frame, while it is up.
    pub(super) fn export_input(&mut self) -> Option<export::Input> {
        let exporting = self.exporting.as_mut()?;
        Some(export::Input {
            source: name_of(&exporting.source),
            name: exporting.name.clone(),
            format: exporting.format,
            quality: exporting.quality,
            resize: exporting.resize.clone(),
            verdict: exporting.verdict.clone(),
            warnings: export::warnings(exporting.facts, exporting.format, &exporting.resize),
            opened: std::mem::take(&mut exporting.opened),
        })
    }

    /// A file has been written at `path`: it joins the list beside the file
    /// on screen and is shown, as a paste is. It arrives with no settings of
    /// its own, which is right — what was done to the picture is in its
    /// pixels now.
    pub(super) fn exported(&mut self, path: PathBuf) {
        self.toast(format!("Exported {}.", name_of(&path)), Level::Message);
        self.from_command_line = false;
        let request = self.files.adopt(path, Source::Disk);
        self.send(request);
        self.list_changed();
    }
}

/// Writes `bytes` to a new file at `to`, never over one: under a temporary
/// name in the same directory first, then moved into place by a rename that
/// refuses to replace. Nothing is left behind if either step fails.
fn write_new(to: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use anyhow::Context;

    let name = name_of(to);
    let temporary = to.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .with_context(|| format!("writing {}", crate::shown_path(&temporary)));
    let placed = written.and_then(|()| match trash::rename_no_replace(&temporary, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(anyhow::anyhow!(TAKEN))
        }
        Err(error) => Err(anyhow::Error::from(error)
            .context(format!("moving the new file to {}", crate::shown_path(to)))),
    });
    if placed.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new file is written, and a file already under the name is left
    /// as it was, with nothing left behind by the attempt.
    #[test]
    fn a_new_file_is_never_written_over_one() {
        let dir = std::env::temp_dir().join(format!("gamut-write-new-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temporary directory is writable");
        let to = dir.join("a.png");
        write_new(&to, b"first").expect("nothing is there yet");
        assert_eq!(std::fs::read(&to).unwrap(), b"first");
        let error = write_new(&to, b"second").expect_err("a.png is taken");
        assert_eq!(error.to_string(), TAKEN);
        assert_eq!(std::fs::read(&to).unwrap(), b"first");
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(left.len(), 1, "no temporary file is left behind");
        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }
}
