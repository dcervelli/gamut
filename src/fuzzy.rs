//! How well a query fits a name, and where: the matcher behind the file
//! chooser.
//!
//! One trait, and the one implementation the program uses. The trait's
//! method is skim's own `FuzzyMatcher::fuzzy_indices`, signature for
//! signature, so that skim's matchers fit it unchanged: the crate this
//! delegates to today is the 2020 extraction of skim's algorithm, and the
//! program skim has kept the same code moving since, under
//! `src/fuzzy_matcher/` of its own tree. Should that ever be wanted here —
//! vendored, MIT, its thread-local caches swapped for a `RefCell` since the
//! chooser matches on one thread — it is a second `impl Matcher` in this
//! file and nothing else changes. This is the only file that names the crate.
//!
//! The chooser's own tests run over `Plain`, a matcher small enough to
//! reason about, so they state what the chooser needs of any matcher rather
//! than what one library happens to score.

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

/// How well a query fits a candidate, and where. The signature is skim's
/// `FuzzyMatcher::fuzzy_indices`, so its matchers fit here unchanged.
pub trait Matcher: Send + Sync {
    /// The score, higher is better, and the char indices of the candidate
    /// the query's chars were found at, in order; `None` when it does not
    /// fit.
    fn fuzzy_indices(&self, choice: &str, pattern: &str) -> Option<(i64, Vec<usize>)>;
}

/// The current implementation: skim's V2 matcher, matching case only where
/// the query has a capital in it.
pub struct Skim(SkimMatcherV2);

impl Skim {
    pub fn new() -> Self {
        Self(SkimMatcherV2::default().smart_case())
    }
}

impl Matcher for Skim {
    fn fuzzy_indices(&self, choice: &str, pattern: &str) -> Option<(i64, Vec<usize>)> {
        self.0.fuzzy_indices(choice, pattern)
    }
}

/// What the application matches with.
pub fn default() -> Box<dyn Matcher> {
    Box::new(Skim::new())
}

/// The smallest matcher that is one: the query's chars found in order,
/// case-insensitively, each at the first place it can be, and the score the
/// negative of the span they cover — so a tighter fit scores higher and an
/// equal fit scores the same.
#[cfg(test)]
pub struct Plain;

#[cfg(test)]
impl Matcher for Plain {
    fn fuzzy_indices(&self, choice: &str, pattern: &str) -> Option<(i64, Vec<usize>)> {
        let mut positions = Vec::new();
        let mut wanted = pattern.chars().flat_map(char::to_lowercase);
        let mut next = wanted.next();
        for (index, c) in choice.chars().enumerate() {
            let Some(want) = next else {
                break;
            };
            if c.to_lowercase().eq(std::iter::once(want)) {
                positions.push(index);
                next = wanted.next();
            }
        }
        if next.is_some() {
            return None;
        }
        let span = match (positions.first(), positions.last()) {
            (Some(first), Some(last)) => (last - first) as i64,
            _ => 0,
        };
        Some((-span, positions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test matcher finds the query's chars in order, leftmost, without
    /// regard to case, and refuses a query that is not there.
    #[test]
    fn plain_finds_a_subsequence_leftmost_and_case_blind() {
        assert_eq!(Plain.fuzzy_indices("abcabc", "ac"), Some((-2, vec![0, 2])));
        // Leftmost, not tightest: the `P` of `Photo` is taken before the
        // one of `PNG`, which is the matcher being simple on purpose.
        assert_eq!(
            Plain.fuzzy_indices("Photo.PNG", "png"),
            Some((-8, vec![0, 7, 8]))
        );
        assert_eq!(Plain.fuzzy_indices("abc", "ca"), None);
        assert_eq!(Plain.fuzzy_indices("abc", ""), Some((0, vec![])));
    }

    /// The adapter is wired: the library finds a query in a name and says
    /// where, and the places it names are places in the name.
    #[test]
    fn skim_is_wired_through_the_trait() {
        let matcher = Skim::new();
        let (score, positions) = matcher
            .fuzzy_indices("photographs/dsc_0417.jpg", "0417")
            .expect("a run of digits in the name is found");
        assert!(score > 0);
        assert_eq!(positions.len(), 4);
        assert!(
            positions
                .iter()
                .all(|&at| at < "photographs/dsc_0417.jpg".chars().count())
        );
        assert_eq!(matcher.fuzzy_indices("a.png", "zzz"), None);
    }
}
