//! The configuration file as TOML: which line a place in the text is on, so
//! that a problem is said on its line; the shapes a value takes, read and
//! named in the words a problem is written in; and a name or a value written
//! back out as TOML, for the template and for a file in the old format
//! rewritten.

use std::ops::Range;

pub(crate) use toml::Spanned;
pub(crate) use toml::de::{DeTable as Table, DeValue as Value};

/// Where each line of a text begins, to say which line a place in it is on.
pub(crate) struct Lines {
    starts: Vec<usize>,
}

impl Lines {
    pub(crate) fn new(text: &str) -> Self {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        Self { starts }
    }

    /// The line, counting from one, that `span` begins on.
    pub(crate) fn of(&self, span: Range<usize>) -> usize {
        self.starts.partition_point(|&start| start <= span.start)
    }
}

/// `text` with each line TOML cannot read blanked, and what was wrong with
/// each, by line: what [`table`] is then given, so that a line that does not
/// read is said and the rest still taken. TOML's own recovery gives up on
/// everything after some mistakes — a name with no `=` after it — so each is
/// blanked in turn and the whole read again, the first mistake each time,
/// since what follows one may only be its echo. Blanked byte for byte, so
/// every place in what is left is where it was in `text`.
pub(crate) fn readable(text: &str) -> (String, Vec<(usize, String)>) {
    let lines = Lines::new(text);
    let mut clean = text.to_string();
    let mut problems = Vec::new();
    for _ in 0..=lines.starts.len() {
        let (_, errors) = Table::parse_recoverable(&clean);
        let Some(error) = errors.first() else {
            break;
        };
        let line = lines.of(error.span().unwrap_or(0..0));
        problems.push((line, error.message().trim_end().to_string()));
        let start = lines.starts[line - 1];
        let end = lines.starts.get(line).map_or(clean.len(), |next| next - 1);
        if clean[start..end].trim().is_empty() {
            // Nothing left on the line to take away: what is wrong is
            // somewhere TOML cannot point to, and the rest is as read.
            break;
        }
        clean.replace_range(start..end, &" ".repeat(end - start));
    }
    (clean, problems)
}

/// `text`, which [`readable`] has made so, parsed.
pub(crate) fn table(text: &str) -> Spanned<Table<'_>> {
    Table::parse_recoverable(text).0
}

/// Every value under `table` that is not itself a table, by its dotted name
/// below it: `zoom.in` for `keys.zoom.in`, however the file nests or quotes
/// the parts.
pub(crate) fn leaves<'a, 'i>(table: &'a Table<'i>) -> Vec<(String, &'a Spanned<Value<'i>>)> {
    let mut found = Vec::new();
    gather(table, "", &mut found);
    found
}

fn gather<'a, 'i>(
    table: &'a Table<'i>,
    prefix: &str,
    found: &mut Vec<(String, &'a Spanned<Value<'i>>)>,
) {
    for (key, value) in table.iter() {
        let name = format!("{prefix}{}", key.get_ref());
        match value.get_ref() {
            Value::Table(inner) => gather(inner, &format!("{name}."), found),
            _ => found.push((name, value)),
        }
    }
}

/// A value as a problem names it: a string, a number or a flag as TOML
/// writes it, an array or a table by what it is.
pub(crate) fn shown(value: &Value) -> String {
    match value {
        Value::String(text) => string(text),
        Value::Integer(number) => number.to_string(),
        Value::Float(number) => number.to_string(),
        Value::Boolean(on) => on.to_string(),
        Value::Datetime(moment) => moment.to_string(),
        Value::Array(_) => "an array".to_string(),
        Value::Table(_) => "a table".to_string(),
    }
}

pub(crate) fn text<'a>(value: &'a Value) -> Option<&'a str> {
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}

pub(crate) fn flag(value: &Value) -> Option<bool> {
    match value {
        Value::Boolean(on) => Some(*on),
        _ => None,
    }
}

/// A whole number, written in any of TOML's bases.
pub(crate) fn whole(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(number) => i64::from_str_radix(number.as_str(), number.radix()).ok(),
        _ => None,
    }
}

/// A finite number, whole or not.
pub(crate) fn number(value: &Value) -> Option<f64> {
    let number = match value {
        Value::Integer(_) => whole(value)? as f64,
        Value::Float(number) => number.as_str().replace('_', "").parse().ok()?,
        _ => return None,
    };
    number.is_finite().then_some(number)
}

/// `text` as a TOML string: between double quotes where nothing in it needs
/// an escape there; between single quotes, which take everything as it is,
/// where it holds a `"` or a `\` but no `'` — a line of the shell, mostly,
/// which reads as it was typed; and escaped between double quotes
/// otherwise.
pub(crate) fn string(text: &str) -> String {
    let control = text.chars().any(char::is_control);
    if !control && !text.contains(['"', '\\']) {
        return format!("\"{text}\"");
    }
    if !control && !text.contains('\'') {
        return format!("'{text}'");
    }
    let mut written = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => written.push_str("\\\""),
            '\\' => written.push_str("\\\\"),
            '\n' => written.push_str("\\n"),
            '\t' => written.push_str("\\t"),
            '\r' => written.push_str("\\r"),
            other if other.is_control() => {
                written.push_str(&format!("\\u{:04X}", u32::from(other)));
            }
            other => written.push(other),
        }
    }
    written.push('"');
    written
}

/// A dotted name as a TOML key: each part bare where TOML takes it bare,
/// and quoted where it does not — `gesture.image."ctrl+middle+wheel"`.
pub(crate) fn key(dotted: &str) -> String {
    dotted
        .split('.')
        .map(|part| {
            let bare = !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if bare { part.to_string() } else { string(part) }
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// The one setting a line of TOML makes, by its dotted name, and its value
/// as text: a string as it is, an array of strings with a space between
/// them. For the tests that read a template's lines back one at a time.
#[cfg(test)]
pub(crate) fn single(line: &str) -> (String, String) {
    let (clean, problems) = readable(line);
    assert_eq!(problems, Vec::new(), "{line}");
    let root = table(&clean);
    let found = leaves(root.get_ref());
    let [(name, value)] = &found[..] else {
        panic!("{line} sets {} things", found.len());
    };
    let value = match value.get_ref() {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| text(item.get_ref()).expect("a string").to_string())
            .collect::<Vec<_>>()
            .join(" "),
        other => shown(other),
    };
    (name.clone(), value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_on_its_line() {
        let lines = Lines::new("a = 1\n\nb = 2\n");
        assert_eq!(lines.of(0..1), 1);
        assert_eq!(lines.of(5..6), 1);
        assert_eq!(lines.of(6..6), 2);
        assert_eq!(lines.of(7..8), 3);
    }

    /// Whatever a string holds, it is read back as it was.
    #[test]
    fn a_string_reads_back_as_it_was() {
        for text in [
            "",
            "plain words",
            "\"$HOME/bin/x\" \"$GAMUT_FILE\"",
            "C:\\path",
            "it's \"both\" \\ here",
            "tab\there\nand a bell \u{7}",
            "#",
        ] {
            let line = format!("a = {}", string(text));
            assert_eq!(single(&line), ("a".to_string(), text.to_string()), "{line}");
        }
        assert_eq!(string("1 0"), "\"1 0\"");
        assert_eq!(string("cp \"$A\" b"), "'cp \"$A\" b'");
    }

    #[test]
    fn a_name_is_quoted_only_where_it_must_be() {
        assert_eq!(key("keys.zoom.100"), "keys.zoom.100");
        assert_eq!(
            key("gesture.image.ctrl+middle+wheel"),
            "gesture.image.\"ctrl+middle+wheel\""
        );
        assert_eq!(
            single(&format!(
                "{} = \"zoom\"",
                key("gesture.image.shift+left.drag")
            ))
            .0,
            "gesture.image.shift+left.drag"
        );
    }

    /// A line TOML cannot read is said and taken away, and every other
    /// line is still read, after it as well as before.
    #[test]
    fn a_line_that_does_not_read_is_taken_away_alone() {
        for (text, kept, said) in [
            ("a = 1\nbad\nb = 2\n", ["a", "b"], 2),
            ("a = 1\nc = yes\nb = 2\n", ["a", "b"], 2),
            ("a = 1\nx.y = 1\nx.y.z = 2\nb = 2\n", ["a", "x"], 3),
            ("a = 1\na = 2\nb = 3\né = \"\n", ["a", "b"], 2),
        ] {
            let (clean, problems) = readable(text);
            assert_eq!(clean.len(), text.len());
            let root = table(&clean);
            let keys: Vec<String> = root
                .get_ref()
                .iter()
                .map(|(key, _)| key.get_ref().to_string())
                .collect();
            for each in kept {
                assert!(keys.iter().any(|key| key == each), "{text:?}: {keys:?}");
            }
            assert_eq!(problems[0].0, said, "{text:?}: {problems:?}");
        }
    }

    #[test]
    fn numbers_read_in_any_base_and_only_finite() {
        let root = table("a = 0x10\nb = 1_000\nc = 2.5\nd = inf\ne = \"3\"\n");
        let get = |name: &str| root.get_ref().get(name).unwrap().get_ref().clone();
        assert_eq!(whole(&get("a")), Some(16));
        assert_eq!(number(&get("b")), Some(1000.0));
        assert_eq!(number(&get("c")), Some(2.5));
        assert_eq!(whole(&get("c")), None);
        assert_eq!(number(&get("d")), None);
        assert_eq!(number(&get("e")), None);
    }
}
