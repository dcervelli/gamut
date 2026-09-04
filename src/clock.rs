//! A moment as the date and time it stands for.
//!
//! Two clocks, because the two places that write one want different things.
//! The information panel says when a file was last written and says it in
//! UTC, so that a time read off the screen — or copied out of the panel —
//! means the same thing to whoever it reaches. A pasted picture is named
//! after the moment it arrived, and that is a local time: it lands in the
//! directory the desktop's own screenshots land in, named the way they are
//! named, and a name a day out from what the clock on the wall said would be
//! no help in finding it again.
//!
//! The standard library knows nothing of time zones, so the local half reads
//! the system's: the compiled zone at `/etc/localtime`, or whichever one
//! `TZ` names. That file is a table of the moments the offset changes, and
//! the answer is the last change at or before the moment asked about. Two
//! things it says are not read here. The abbreviation — `NZST` — is not a
//! number and nothing wants it; and the rule at the end of the file, which
//! says what happens after the last change the table records, is not parsed,
//! so a moment past the end of the table keeps the offset the table left off
//! at. The tables distributions ship run to 2037.
//!
//! A zone that cannot be read at all leaves the offset at zero, which is to
//! say local time falls back to UTC rather than to a guess.

use std::path::PathBuf;
use std::time::SystemTime;

/// A moment, split into the fields a date is written from. Which zone it is
/// in is settled by whether it came from [`utc`] or from [`local`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// What a clock in UTC said at `time`.
pub fn utc(time: SystemTime) -> Civil {
    civil(epoch_seconds(time))
}

/// What the clock on this machine's wall said at `time`.
pub fn local(time: SystemTime) -> Civil {
    let seconds = epoch_seconds(time);
    civil(seconds + offset_at(seconds))
}

/// `time` as seconds since the epoch, which is what a zone is a table of.
fn epoch_seconds(time: SystemTime) -> i64 {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => since.as_secs() as i64,
        // Before 1970, which a file can be: the error carries how far before.
        Err(before) => -(before.duration().as_secs() as i64),
    }
}

/// The date and time `seconds` after the epoch, in whatever zone that count
/// has already been shifted into.
fn civil(seconds: i64) -> Civil {
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        hour: (time_of_day / 3600) as u32,
        minute: ((time_of_day / 60) % 60) as u32,
        second: (time_of_day % 60) as u32,
    }
}

/// The civil date `days` after 1970-01-01, by Howard Hinnant's algorithm:
/// the calendar is shifted to start in March so that the leap day falls at
/// the end of the year and the month lengths make a repeating pattern.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// How far ahead of UTC this machine's clock was at `at`, in seconds. Zero
/// where there is no zone to be read, which leaves local time reading as UTC
/// rather than as a guess.
fn offset_at(at: i64) -> i64 {
    std::fs::read(zone_file())
        .ok()
        .and_then(|bytes| offset_in(&bytes, at))
        .unwrap_or(0)
}

/// Which compiled zone to read.
///
/// `TZ` naming one is honoured, since a program run under it is being asked
/// for that zone's time. Only the form that names a zone — `Pacific/Auckland`,
/// with or without the leading colon a shell convention allows — is read: a
/// `TZ` spelling the rule out in full is a grammar of its own, and one this
/// does not undertake to parse falls back to the system's zone rather than
/// to something invented. A name that could climb out of the zone directory
/// is refused for the same reason any path from the environment is.
fn zone_file() -> PathBuf {
    const SYSTEM: &str = "/etc/localtime";
    const ZONES: &str = "/usr/share/zoneinfo";

    let Some(name) = std::env::var_os("TZ") else {
        return PathBuf::from(SYSTEM);
    };
    let Some(name) = name
        .to_str()
        .map(|name| name.strip_prefix(':').unwrap_or(name))
    else {
        return PathBuf::from(SYSTEM);
    };
    let named = PathBuf::from(ZONES).join(name);
    if name.is_empty()
        || name.starts_with('/')
        || name.split('/').any(|part| part == "." || part == "..")
        || !named.is_file()
    {
        return PathBuf::from(SYSTEM);
    }
    named
}

/// Reads a TZif file — RFC 8536 — for the offset in force at `at`, and
/// `None` for anything it cannot make sense of. Every read is bounds-checked
/// against the length actually there rather than against the counts the file
/// claims, since the counts are as much a part of the file as the data.
fn offset_in(bytes: &[u8], at: i64) -> Option<i64> {
    let mut reader = Reader { bytes, at: 0 };
    let counts = header(&mut reader)?;
    if counts.version < b'2' {
        return block(&mut reader, &counts, 4, at);
    }
    // The whole file again, with room for the times a 32-bit count cannot
    // reach. The first copy is skipped rather than read: it stops in 2038,
    // and everything in it is in the second copy as well.
    reader.skip(block_length(&counts, 4))?;
    let counts = header(&mut reader)?;
    block(&mut reader, &counts, 8, at)
}

/// The header's six counts, and the version that says how wide the times in
/// the block after it are.
struct Counts {
    version: u8,
    ut_indicators: usize,
    standard_indicators: usize,
    leap_seconds: usize,
    transitions: usize,
    types: usize,
    designations: usize,
}

fn header(reader: &mut Reader) -> Option<Counts> {
    if reader.take(4)? != b"TZif" {
        return None;
    }
    let version = reader.take(1)?[0];
    reader.skip(15)?;
    Some(Counts {
        version,
        ut_indicators: reader.count()?,
        standard_indicators: reader.count()?,
        leap_seconds: reader.count()?,
        transitions: reader.count()?,
        types: reader.count()?,
        designations: reader.count()?,
    })
}

/// How long the data block described by `counts` is, with times `width` bytes
/// wide. A leap second is a time and a count of them, so it widens too.
fn block_length(counts: &Counts, width: usize) -> usize {
    counts.transitions * (width + 1)
        + counts.types * 6
        + counts.designations
        + counts.leap_seconds * (width + 4)
        + counts.standard_indicators
        + counts.ut_indicators
}

/// The offset in force at `at`, read out of the data block the reader is
/// standing at the start of.
fn block(reader: &mut Reader, counts: &Counts, width: usize, at: i64) -> Option<i64> {
    let mut transitions = Vec::new();
    for _ in 0..counts.transitions {
        transitions.push(reader.time(width)?);
    }
    let which = reader.take(counts.transitions)?.to_vec();

    let mut offsets = Vec::new();
    let mut standard = None;
    for index in 0..counts.types {
        offsets.push(i64::from(reader.signed()?));
        let daylight = reader.take(1)?[0] != 0;
        // The index into the abbreviations, which nothing here wants: what a
        // zone is called says nothing about what the clock reads.
        reader.skip(1)?;
        if !daylight && standard.is_none() {
            standard = Some(index);
        }
    }

    // The transitions are in order, so the one in force is the last that has
    // already happened. Before the first — or in a zone that has never
    // changed offset — the file's own answer is its first standard type.
    let past = transitions.partition_point(|&when| when <= at);
    let kind = match past.checked_sub(1) {
        Some(last) => usize::from(*which.get(last)?),
        None => standard.unwrap_or(0),
    };
    offsets.get(kind).copied()
}

/// A position in the bytes, with every read answering `None` rather than
/// reaching past the end of them.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(slice)
    }

    fn skip(&mut self, count: usize) -> Option<()> {
        self.take(count).map(|_| ())
    }

    /// One of the header's counts, as somewhere to index.
    fn count(&mut self) -> Option<usize> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?) as usize)
    }

    /// A signed 32-bit quantity: an offset from UTC, in seconds.
    fn signed(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    /// A moment, as wide as the block it is in.
    fn time(&mut self, width: usize) -> Option<i64> {
        match width {
            8 => Some(i64::from_be_bytes(self.take(8)?.try_into().ok()?)),
            _ => self.signed().map(i64::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(seconds: u64) -> Civil {
        utc(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
    }

    #[test]
    fn the_epoch_and_a_moment_after_it() {
        assert_eq!(
            at(0),
            Civil {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0
            }
        );
        assert_eq!(
            at(1_756_632_722),
            Civil {
                year: 2025,
                month: 8,
                day: 31,
                hour: 9,
                minute: 32,
                second: 2
            }
        );
    }

    /// The leap day the shifted calendar exists to get right, and the century
    /// year that is not a leap year at all.
    #[test]
    fn leap_days_land_where_they_should() {
        assert_eq!((at(951_782_400).month, at(951_782_400).day), (2, 29));
        let before = utc(SystemTime::UNIX_EPOCH - Duration::from_secs(2_203_891_200));
        assert_eq!((before.year, before.month, before.day), (1900, 3, 1));
    }

    /// A file older than the epoch still has a date, which is the half of
    /// `duration_since` that returns an error.
    #[test]
    fn a_time_before_the_epoch_counts_backwards() {
        let before = utc(SystemTime::UNIX_EPOCH - Duration::from_secs(86_400));
        assert_eq!((before.year, before.month, before.day), (1969, 12, 31));
    }

    /// A TZif file as `zic` writes one: the header, a version-1 block that a
    /// reader of this version is meant to skip, and the same zone again with
    /// 64-bit times. The rule that would follow the second block is left off,
    /// this not being a reader of it.
    fn tzif(types: &[(i32, bool)], transitions: &[(i64, u8)]) -> Vec<u8> {
        let mut file = Vec::new();
        // The version-1 block, carrying the types and none of the
        // transitions — enough to be skipped over correctly, which is what
        // the counts in its header are being tested for.
        file.extend(header_bytes(types.len(), 0, types.len()));
        file.extend(type_bytes(types));
        file.extend(b"LMT\0");
        file.extend(vec![1u8; types.len() * 2]);

        file.extend(header_bytes(types.len(), transitions.len(), 0));
        for (when, _) in transitions {
            file.extend(when.to_be_bytes());
        }
        for (_, which) in transitions {
            file.push(*which);
        }
        file.extend(type_bytes(types));
        file.extend(b"LMT\0");
        file
    }

    fn header_bytes(types: usize, transitions: usize, indicators: usize) -> Vec<u8> {
        let mut header = Vec::from(b"TZif2");
        header.extend([0u8; 15]);
        for count in [indicators, indicators, 0, transitions, types, 4] {
            header.extend((count as u32).to_be_bytes());
        }
        header
    }

    fn type_bytes(types: &[(i32, bool)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (offset, daylight) in types {
            bytes.extend(offset.to_be_bytes());
            bytes.push(u8::from(*daylight));
            bytes.push(0);
        }
        bytes
    }

    /// A zone that changes offset twice a year: the answer is the last change
    /// that has already happened.
    #[test]
    fn the_offset_is_the_last_change_at_or_before_the_moment() {
        let standard = 12 * 3600;
        let daylight = 13 * 3600;
        let zone = tzif(
            &[(standard, false), (daylight, true)],
            &[(1_000, 1), (2_000, 0), (3_000, 1)],
        );

        assert_eq!(offset_in(&zone, 1_500), Some(i64::from(daylight)));
        assert_eq!(
            offset_in(&zone, 2_000),
            Some(i64::from(standard)),
            "the moment of a change is already after it"
        );
        assert_eq!(offset_in(&zone, 2_999), Some(i64::from(standard)));
        assert_eq!(
            offset_in(&zone, 4_000),
            Some(i64::from(daylight)),
            "past the last change the table records, it keeps the offset it left off at"
        );
        assert_eq!(
            offset_in(&zone, 0),
            Some(i64::from(standard)),
            "before the first, the zone's standard offset"
        );
    }

    /// The system's own zone, read the way anything else reads it. Skipped
    /// where there is none — a container without `tzdata` — since the point
    /// is that a real file agrees, not that every machine has one.
    #[test]
    fn the_system_zone_agrees_with_what_the_system_says_the_time_is() {
        let Ok(bytes) = std::fs::read("/etc/localtime") else {
            return;
        };
        let now = epoch_seconds(SystemTime::now());
        let offset = offset_in(&bytes, now).expect("the system's zone reads");
        assert!(
            offset.abs() <= 26 * 3600 && offset % 60 == 0,
            "{offset} seconds is not an offset any zone has"
        );

        // And the local clock is the UTC one moved by exactly that much.
        let moved = utc(SystemTime::UNIX_EPOCH + Duration::from_secs((now + offset) as u64));
        assert_eq!(local(SystemTime::now()).hour, moved.hour);
    }

    /// A zone that has never changed offset has no transitions at all, and
    /// the whole of its answer is its one type.
    #[test]
    fn a_zone_that_never_changes_still_answers() {
        let zone = tzif(&[(-5 * 3600, false)], &[]);
        assert_eq!(offset_in(&zone, 1_756_632_722), Some(-5 * 3600));
    }

    /// The file is read from the filesystem and is not this program's own, so
    /// nothing in it may be taken on trust: a header that is not one, a file
    /// that stops in the middle, and counts that promise more than the file
    /// holds all have to come back as an answer rather than as a panic.
    #[test]
    fn a_file_that_is_not_a_zone_is_refused_rather_than_trusted() {
        assert_eq!(offset_in(b"", 0), None);
        assert_eq!(offset_in(b"not a zone file at all", 0), None);

        let zone = tzif(&[(3600, false)], &[(1_000, 0)]);
        // Up to the last of the offsets, which is the last thing read: the
        // abbreviations behind them are not, so a file that stops there is
        // still a file this can answer from.
        let needed = zone.len() - b"LMT\0".len();
        for cut in 0..needed {
            assert_eq!(offset_in(&zone[..cut], 1_500), None, "cut at {cut}");
        }
        assert_eq!(offset_in(&zone, 1_500), Some(3600), "and whole, it reads");

        // A transition count of four billion, with nothing behind it.
        let second = zone
            .windows(4)
            .enumerate()
            .skip(1)
            .find(|(_, magic)| *magic == b"TZif")
            .map(|(at, _)| at)
            .expect("a version-2 file carries its zone twice");
        let count = second + 20 + 12;
        let mut lying = zone.clone();
        lying[count..count + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(offset_in(&lying, 1_500), None);
    }
}
