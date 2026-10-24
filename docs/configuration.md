# The configuration file

`~/.config/gamut/config.toml` is the user's: how the window opens, what the
keys and the mouse do, and the commands. What each setting does, and how a
user writes one, is in [`user-docs/SETTINGS.md`](../user-docs/SETTINGS.md);
this page is why it is read as it is. `src/settings/mod.rs` holds `Config`
and its parse, `src/settings/document.rs` what reading TOML needs, and
`src/settings/legacy.rs` the rewrite of a file in the old format.

## TOML, read by hand

Flat `name = value` lines hold the settings, the keys and the gestures
well and the commands badly: a command would be a run of
`command.<name>.<field>` lines restating its name on every one, an input's
kind a small grammar of its own, a `#` after a space a comment even inside
a line of the shell, and nothing could span lines. TOML gives a command a
table, its inputs an array of tables with typed fields, a line of the shell
a quoted string that can hold anything, and a script several lines, while a
setting, a key and a gesture stay one line each — a user already editing
other programs' TOML meets nothing new.

The `toml` crate is used for its parser alone, without serde:
`DeTable::parse_recoverable` hands back the document as tables of values,
each key and value with its byte span, and everything is read from those by
hand — `Setting::set` for a setting, `Config::parse_keys` and
`Config::parse_gesture`, `commands::parse` for the commands. Deriving the
configuration would refuse a whole file over one bad value; read by hand,
each value that does not do is said on its own line, from the span through
`document::Lines`, and the rest is still taken, as the old format did. The
`preserve_order` feature keeps the tables in the file's order, which is the
order the commands are listed in.

TOML's own recovery from a syntax error is partial: after some mistakes — a
name with no `=` — it gives up on the rest of the file. `document::readable`
takes the first error, blanks its line byte for byte so every span stays
where it was, and parses again, until nothing is wrong; then `table` parses
what is left. A file with a mistake in it costs the line the mistake is on,
as it did.

## Names that are dotted keys

A key's name and a gesture's slot are written as TOML's dotted keys:
`keys.zoom.in` is the table `keys`, the table `zoom` in it, and `in`.
`document::leaves` flattens the tables back into the dotted name, so the
file may nest or quote them however TOML allows — `keys.zoom.in`,
`[keys] zoom.in`, `keys."zoom.in"` are one name.

Two things follow. A part of a name that is not letters, digits, `-` and
`_` must be quoted, so a gesture's `shift+left` is written
`gesture.image."shift+left".drag`; `document::key` quotes exactly the parts
that need it when the template is written. And no name can be the start of
another: `zoom.100` would be both a string and the table holding
`zoom.100.toggle`, which TOML refuses. The table's names were renamed where
that happened — `zoom.toggle`, `files.map-bounds` — and
`the_template_is_every_default`, which reads every name of the template back
uncommented, fails on any name added that starts another.

A TOML table takes every line after its heading, so a `keys.` line written
below a `[command.<name>]` lands in the command as a field named `keys`.
`settings::top_level` lets the command's reader say so in those words rather
than calling it an unknown field.

## A file in the old format

`Config::load` reads `config.toml`; where there is none and the old
`config` is there, `legacy::rewrite` turns it into TOML and `write_new`
writes it beside it — through a temporary name and a rename that refuses to
replace, so two windows rewriting at once cannot clobber each other — and
the window says it was done. The old file is left as it was: deleting it
would lose nothing the new one lacks, but it is the user's.

The rewrite goes line by line rather than parsing and writing back out, so
that a user's comments stay where they wrote them. A setting is written as
TOML writes it in its place, flags and whole numbers bare where the value
was one; a setting commented out is rewritten too, so that taking its `#`
off still works; a comment that is words is kept as it is. The passages of
the template whose words the new format made wrong are recognized as they
were printed and put back as the template has them now. The commands move to
the foot of the file, each a table, since a table takes the lines after it;
an input's old grammar is read by `legacy::read_kind` into the same `Kind`
the TOML reader makes, and written out as its fields. A name set twice keeps
the later line, as the old reader did, the earlier one commented out, since
TOML takes a name once. A line that did not read before is carried over as
it was, so it does not read now either and is said as it was.

Where the new file cannot be written — a read-only directory — the rewritten
text is read in memory instead, every start, and the problems are said
against the old file's lines through `Rewritten::origin`.
