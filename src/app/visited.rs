//! The files that have been on screen, in the order they were, and where
//! in that order the one on screen is: what going back and forward
//! through them steps along.
//!
//! A browser's history rather than a list of everything seen. Going back
//! and then somewhere new cuts off what was ahead, so that forward always
//! leads to something reached from here; going back and then forward
//! retraces the step. A file that has left the list — taken off it, or
//! moved to the trash — is skipped rather than struck out: undo can put it
//! back, and it would be strange for it to have fallen out of the past in
//! the meantime.
//!
//! Pure, and told about arrivals rather than asking for them: the file
//! goes on the stack when it reaches the screen, whatever asked for it,
//! and a back or forward marks where it is heading so that the arrival it
//! asked for is a move along the stack rather than a new entry on it.

use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct Visited {
    paths: Vec<PathBuf>,
    /// Where in `paths` the file on screen is. `None` before anything has
    /// arrived.
    at: Option<usize>,
    /// Where a back or forward asked to go, until its file arrives.
    heading: Option<usize>,
}

impl Visited {
    /// A file has reached the screen. The one a back or forward was heading
    /// for moves the cursor there; any other cuts off whatever lay ahead
    /// and goes on the end — unless it is the file already there, which a
    /// reload or a page turn brings round again.
    pub(super) fn arrived(&mut self, path: &Path) {
        if let Some(heading) = self.heading.take()
            && self.paths.get(heading).is_some_and(|held| held == path)
        {
            self.at = Some(heading);
            return;
        }
        if let Some(at) = self.at
            && self.paths[at] == path
        {
            return;
        }
        let keep = self.at.map_or(0, |at| at + 1);
        self.paths.truncate(keep);
        self.paths.push(path.to_path_buf());
        self.at = Some(self.paths.len() - 1);
    }

    /// The nearest file before the one on screen that is still on the list
    /// — `listed` says which are — marked as where we are heading. `None`
    /// with nothing to go back to.
    pub(super) fn back(&mut self, listed: impl Fn(&Path) -> bool) -> Option<PathBuf> {
        let heading = self.nearest(false, &listed)?;
        self.heading = Some(heading);
        Some(self.paths[heading].clone())
    }

    /// The nearest file after the one on screen that is still on the list,
    /// marked as where we are heading. `None` with nothing to go forward to.
    pub(super) fn forward(&mut self, listed: impl Fn(&Path) -> bool) -> Option<PathBuf> {
        let heading = self.nearest(true, &listed)?;
        self.heading = Some(heading);
        Some(self.paths[heading].clone())
    }

    /// Whether there is a file to go back to.
    pub(super) fn can_back(&self, listed: impl Fn(&Path) -> bool) -> bool {
        self.nearest(false, &listed).is_some()
    }

    /// Whether there is a file to go forward to.
    pub(super) fn can_forward(&self, listed: impl Fn(&Path) -> bool) -> bool {
        self.nearest(true, &listed).is_some()
    }

    /// The nearest entry on the given side of the cursor that is still on
    /// the list.
    fn nearest(&self, forward: bool, listed: &impl Fn(&Path) -> bool) -> Option<usize> {
        let at = self.at?;
        let mut candidates: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(at + 1..self.paths.len())
        } else {
            Box::new((0..at).rev())
        };
        candidates.find(|&index| listed(&self.paths[index]))
    }

    /// The file at `from` is called `to` now, wherever it is in the past.
    pub(super) fn rename(&mut self, from: &Path, to: &Path) {
        for held in &mut self.paths {
            if held == from {
                *held = to.to_path_buf();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(_: &Path) -> bool {
        true
    }

    /// Each arrival goes on the end; back and forward step along what has
    /// arrived; and a new arrival after going back cuts off what lay
    /// ahead, as a browser's history does.
    #[test]
    fn arrivals_stack_up_and_a_new_one_after_going_back_cuts_off_the_rest() {
        let mut visited = Visited::default();
        assert!(!visited.can_back(all) && !visited.can_forward(all));
        visited.arrived(Path::new("a"));
        visited.arrived(Path::new("b"));
        visited.arrived(Path::new("c"));
        assert!(visited.can_back(all));
        assert!(!visited.can_forward(all));

        assert_eq!(visited.back(all), Some(PathBuf::from("b")));
        visited.arrived(Path::new("b"));
        assert!(visited.can_forward(all));
        assert_eq!(visited.back(all), Some(PathBuf::from("a")));
        visited.arrived(Path::new("a"));
        assert!(!visited.can_back(all), "at the start");
        assert_eq!(visited.forward(all), Some(PathBuf::from("b")));
        visited.arrived(Path::new("b"));
        assert_eq!(visited.forward(all), Some(PathBuf::from("c")));
        visited.arrived(Path::new("c"));
        assert!(!visited.can_forward(all), "at the end");

        assert_eq!(visited.back(all), Some(PathBuf::from("b")));
        visited.arrived(Path::new("b"));
        visited.arrived(Path::new("d"));
        assert!(!visited.can_forward(all), "c was cut off");
        assert_eq!(visited.back(all), Some(PathBuf::from("b")));
        visited.arrived(Path::new("b"));
        assert_eq!(visited.forward(all), Some(PathBuf::from("d")));
    }

    /// The same file arriving again — a reload, a page turn — is not a
    /// second entry, and a back that is answered by something else is
    /// forgotten: the arrival is what counts.
    #[test]
    fn a_file_arriving_again_and_a_heading_not_reached_leave_no_trace() {
        let mut visited = Visited::default();
        visited.arrived(Path::new("a"));
        visited.arrived(Path::new("a"));
        assert!(!visited.can_back(all));
        visited.arrived(Path::new("b"));
        assert_eq!(visited.back(all), Some(PathBuf::from("a")));
        // A pick from the chooser lands first.
        visited.arrived(Path::new("c"));
        assert!(
            !visited.can_forward(all),
            "b was cut off by the pick, not by the back"
        );
        assert_eq!(visited.back(all), Some(PathBuf::from("b")));
    }

    /// A file that has left the list is stepped over rather than struck
    /// out, and is there to go back to again once it is on the list again.
    #[test]
    fn a_file_off_the_list_is_skipped_until_it_is_back() {
        let mut visited = Visited::default();
        for name in ["a", "b", "c"] {
            visited.arrived(Path::new(name));
        }
        let without_b = |path: &Path| path != Path::new("b");
        assert_eq!(visited.back(without_b), Some(PathBuf::from("a")));
        visited.arrived(Path::new("a"));
        assert_eq!(visited.forward(without_b), Some(PathBuf::from("c")));
        visited.arrived(Path::new("c"));
        assert_eq!(
            visited.back(all),
            Some(PathBuf::from("b")),
            "back on the list"
        );
        let only_c = |path: &Path| path == Path::new("c");
        assert!(!visited.can_back(only_c));
    }

    /// A rename follows the file into the past.
    #[test]
    fn a_renamed_file_is_known_by_its_new_name() {
        let mut visited = Visited::default();
        visited.arrived(Path::new("a"));
        visited.arrived(Path::new("b"));
        visited.rename(Path::new("a"), Path::new("z"));
        assert_eq!(visited.back(all), Some(PathBuf::from("z")));
    }
}
