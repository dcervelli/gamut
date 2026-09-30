# Every tag: the Raw Data tab

The information panel's first tab, *Curated* (`ui::tags::Tab::Facts` in the
code), is curated on purpose — see [the interface](interface.md) and
`src/image/exif.rs`'s module doc. The second, *Raw Data*
(`ui::tags::Tab::Tags`, called the Tags tab here and in the code), is the
other half: every tag `exiftool` can read from the file, maker
notes included, unedited. What a user sees of it is in
[`user-docs/METADATA.md`](../user-docs/METADATA.md#every-tag).

## A process, not a library

`exiftool` reads more formats and more makers' notes than any reader in this
tree will, and keeps up with new cameras on its own schedule. Rather than
grow the readers towards it, `src/exiftool.rs` runs it as a subprocess and
reads what it prints. It is a Perl program the user installs; where it is
not installed the tab says how to install it, and nothing else in the
program depends on it.

It runs only while the tab is on screen: when the tab is switched to, when
the panel comes up already on it, and for each file `App::apply` puts on
screen while it is up. That last includes a file read again after a write,
so the tab follows the file with one call. A run takes about 50 ms on a
small file and a few hundred on a large raw.

## What is asked for, and why

```
exiftool -X -l -D -t -G1 -a -u -struct -api LargeFileSupport=1 /absolute/path
```

- `-X` is RDF/XML. `roxmltree`, already here for the XMP packet
  (`src/image/xmp.rs`), parses it, and it keeps a tag that appears twice —
  two `IFD0:Orientation` in a malformed file are both shown. `-j` would drop
  the second, and would need a JSON crate the tree does not otherwise have.
- `-l` adds, per tag, `et:desc` (the description), `et:prt` (the printed
  value) and `et:val` (the raw value, only where it differs), which become
  `Tag::desc`, `Tag::printed` and `Tag::raw`. `-s` is not passed: it takes
  those three away, and the element's local name is the tag's name anyway.
- `-D` and `-t` put `et:id` and `et:table` on an `rdf:Description` wrapper
  inside each tag's element: `Tag::id` and `Tag::table`. A language
  alternative arrives as its own tag, `XMP-dc:Title-fr`, with `xml:lang` on
  the wrapper: `Tag::lang`.
- `-G1` names each tag's XML prefix by its family-1 group, and its namespace
  URI carries both families: `http://ns.exiftool.org/EXIF/IFD0/1.0/` is
  `EXIF` and `IFD0`. `Composite` and `ExifTool` have one segment, which is
  both. The tree is those two levels, in the order each group first appears;
  a family-0 group whose every tag is in a family-1 group of its own name is
  not split, so `Composite` is one level.
- `-a` keeps duplicates and `-u` unknown tags, since the tab's point is
  everything.
- `-struct` writes a structure as one: a list is `rdf:Bag`, `rdf:Seq` or
  `rdf:Alt` of `rdf:li` *inside* `et:prt`, and a structure is `et:prt
  rdf:parseType='Resource'` with a child element per field, prefixed with the
  tag's own group; an `rdf:li` may itself be a resource. `exiftool::Value`
  is that shape — `Text`, `List`, `Struct`. `Value::leaves` walks it to
  every piece of text by dotted path, `RegionList.1.Name`, for the filter;
  `Value::pieces` does the same but keeps a list of text whole, written
  `[1, 2, 3]` by `Value::summary`, for the rows under a tag and the copies.
  A calibration matrix is nine numbers in an `rdf:Seq`, and is one value to
  read, not nine rows.
- Binary values arrive as the text `(Binary data N bytes, use -b option to
  extract)`. Nothing binary crosses the pipe.

The path is made absolute with `std::path::absolute`, so it cannot be read
as an option, but not canonicalized, so a missing file is exiftool's to
report. There is no shell. A damaged file exits 0 with `ExifTool:Warning`
tags, which the tab shows like any other; stderr is read only on a non-zero
exit, whose first line is what `Failure::Failed` carries and the tab says.
The version is `et:toolkit`'s.

## Finding the program

The `exiftool` setting is a bare name or a path. A path is used as it is
when it is a file. A bare name is looked for in `PATH`, then in the
directories a desktop-launched program's `PATH` misses: Arch puts Perl
scripts in `/usr/bin/vendor_perl`, and a Mac app started from Finder or the
Dock inherits none of the shell's environment, so Homebrew's
`/opt/homebrew/bin` and `/usr/local/bin` and MacPorts' `/opt/local/bin` are
probed there. `Program::locate` looks again each time the tab asks while
nothing has been found, so installing it while the program runs works; a
spawn failing with `NotFound` makes it look again next time too.
`App::reconfigure` replaces it when the setting changes, and when it has
not been found under the same setting, reading the file at once if the tab
is waiting; the tab's Refresh button does the same without a reload. While
it is not found the tab offers its website, the configuration file and
Refresh, and its copy button is dead through `Conditions::tags_in`.

## Asking, and keeping

`app/tags.rs::Tags` is modeled on `app/measuring.rs`. Each run is a
`Request` numbered by `asked`, run on a detached thread of its own
(`exiftool::run_on_thread`), and answered as `UserEvent::Tags`. An answer is
kept per file under the file's `watch::Signature` — its length and
modification time — so stepping back to a file shows its tags at once and a
file rewritten since is read again. An answer to a run that is no longer the
one waited for is kept all the same, but ends no wait. At most
`MAX_REPORTS` files are kept, least recently wanted first to go. That the
program is not installed is never kept as an answer: the next look may find
it.

There is no timeout. A run on a file on a hung network mount waits for as
long as the mount does, and the tab shows its spinner; stepping to another
file asks for that file, and the hung run's answer is dropped when it
lands. A timeout that killed the child is a possible follow-up.

## A tab, not a popup

The tags are about the file on screen and are read beside it, which is what
the information panel already is; a popup would cover the picture and take
the keyboard. For the same reason the filter field is not given the keyboard
when the tab comes up — a focused field takes every key the window answers —
but by a click, and `Esc` hands it back (read in the pass before the field,
as the chooser reads its keys). The tab does not survive a restart: the
panel opens on its facts.

With no filter the rows are the tree, in the file's own order. Every group
starts shut — a raw's maker notes alone would otherwise bury the rest —
and what is opened is kept for the session as a default and the groups
flipped from it (`Tags::open_by_default`, `Tags::flipped`), so that the
button that opens or folds every group applies to the next file too. A filter makes them one list, best first,
each tag's group written before its name since there is no tree to say it —
`app/tags.rs::ranked`. The filter is matched against the group, the name,
the value, the description, the raw value and the ID. Over a few hundred
such candidates the chooser's fuzzy matcher (`src/fuzzy.rs`) fits nearly
any short query somewhere, and its scores barely part a scattered hit from
a tag named by the query, so the ranking is in tiers: a tag whose title —
exiftool's description, which heads its row, or its name where there is
none — holds every word of the query whole, then one whose candidate does
(the name included), each lit
where the words are, and the matcher's score orders within a tier. The
matcher's scattered hits are kept only where no tag holds the words, so a
misspelling still finds something.

## Rows of two heights

A raw's maker notes run to hundreds of tags, so the tree is virtualized:
`ui::tags::tops` works out where each row starts when the rows are built —
a group one line tall, a tag or a piece of one two — and a pass lays out
only the rows between the viewport's ends, found by binary search. The rows
are rebuilt only when the query, the folding or the answer changes, and are
shared with the frame rather than copied into it.

## Copies

A click on a row of the tree copies its value as the row writes it, a list
of text as `[a, b]`, and anything deeper a line per piece, and marks the
row. A click on a query's hit copies nothing — it is the way from the list
back to the tree, and a copy would overwrite what the clipboard held for a
click meant to look: `Tags::choose`
clears the query, flips open the tag's two groups where they are shut and
touches no other, and has the next frame scroll the marked row to the
middle of the list (`Input::reveal`, taken once, as the chooser's is).

The Copy menu's items copy every tag that fits the filter, folded or not —
the count line says how many. Plaintext and CSV are made from flat lines
(`ui::tags::text`, `ui::tags::csv`); JSON and XML from the tags themselves
(`app/tags.rs::json`, `xml`), so a list stays an array or `item`s and a
structure an object or `field`s. Both are written by hand — there is no
serializer in the tree — and each escapes what its syntax needs. CSV has the heads `Group,Subgroup,Tag,ID,Description,Value,Raw`,
a row per tag and a row per piece of a structure with its path after
the tag's name, quoted as the Curated tab quotes (`ui::info::quoted`). Text is
exiftool's own `-G1` layout, `[IFD0]         Orientation                     : Rotate 180 (3)`,
with the raw value in parentheses where it differs.
