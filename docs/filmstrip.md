# The file list

`Tab` puts a strip of thumbnails down the left of the picture: the list,
one row per file, in the order the keys walk it. This page is about why
the order it shows is the list's own, how the list is put in that order
without disturbing a read, what the strip is drawn from, and the two
things beside it — the files that have been on screen, and a file taken
off the list. The keys and what a user sees are in
[`user-docs/KEYS.md`](../user-docs/KEYS.md#the-file-list).

## The order is the list's own

The strip could have been a view: a permutation kept beside `Files`, drawn
in that order and stepped through by translating each `]` into an index.
It is not. `App::apply_order` puts `Files::paths` itself in the order the
strip asks for, through `Files::reorder`, and the strip draws the list as
it stands. Everything that walks the list — the keys, `Files::step_away`
after a deletion, the counter in the bar, the chooser's `:12` — then
agrees about what comes next, and none of it has to know that an order
exists. The cost is that `Files` has to keep the file on screen the file
on screen across a reorder, which it does by re-finding its path, exactly
as `Files::relist` already did for a rebuild.

The order is `ui::filmstrip::Order`: a `Section` — none, the file's
directory, or its type — a `Sort` within each section, and the
`Direction` the sort runs in, which turns the comparison round rather
than the list, so that ties keep their order either way and the sections
and the unknown stay where they were. `app/order.rs`
turns one into a permutation, `arrange`, and says where the sections fall
in a list already arranged, `groups`. It compares `(section, key)` with a
`None` on either side sorting after every `Some`, so that a file whose
header has not been read yet — its type, its size on disk and its
dimensions all come from `thumbnailer::Facts`, which arrive in the
background — goes to the end and moves into place when its header does. The
sort is `slice::sort_by` on indices, which is stable, and the list is
sorted in place from wherever it stands, so files one key cannot tell
apart keep the order the last key put them in: sorting by size and then by
type leaves each type in size order, as sorting a spreadsheet twice does.
That is also why `Files::relist` keeps the order the list stands in rather
than taking the rebuild's: a directory is read in name order, and a rebuild
that came back in it would undo every sort on every look. Survivors keep the order they stand in, and
a newcomer goes in after the nearest file the rebuild lists before it,
which for a list in name order is where the directory has it.

The two enums live in `ui/` rather than `app/` because the menus at the
strip's head offer them and wear their words, and `ui/` cannot import
`app/` — the same split as `Copies` and `copy_action`.

## Between reads, and once per poll

A request in flight is aimed at an index, in `Files::Pending` and in the
loader's `Opened`, so the list cannot move under one. `apply_order`
refuses while `Files::is_idle` is false and marks the strip stale instead;
`App::poll_order` applies a stale order at the next poll, and
`App::deliver` tries again the moment a read lands, so a sort asked for
under a read waits exactly as long as the read. The same flag is what
coalesces the headers: every `News::Facts` the thumbnail thread delivers
could change where a file sorts, and on a list of thousands that is
thousands of arrivals in the first seconds, so `App::facts_learned` marks
the strip stale — only when the order reads facts at all — and the list is
sorted once per poll rather than once per header. `App::list_changed`
marks it stale too, for a rebuild that came back merged and for a file put
back by undo, which `Files::reinstate` lands at its old index; and
`App::poll_directories` applies the order at once after a rebuild, being
idle by construction.

## What the strip is drawn from

`app/filmstrip.rs` is the strip's state and `ui/filmstrip.rs` the panel,
the way `app/chooser.rs` and `ui/chooser.rs` are the chooser's. The rows —
a `Row::Header` for each section where the list is sectioned, a
`Row::File` under it for each file — are rebuilt only when the list, a
header or a thumbnail changed, and handed to the frame as an `Arc<[Row]>`.
Beside them goes `tops`, where each row starts down the strip: rows are
two heights, and a prefix sum lets `ui::filmstrip::span` find the rows a
viewport touches by binary search rather than by measuring every row above
it. Only those rows are laid out, and which they are goes back as
`Command::FilmstripVisible` when it changes, which is how their
thumbnails go to the front of the thread's queue and how `Thumbs` — the
one store, shared with the chooser and described in
[the chooser](chooser.md#what-the-screen-holds) — knows they are on
screen. A row is pressed as `Control::Thumb(row)` and resolved to its path
and then to its place, never by index, since the list is free to have
moved between the frame and the press.

The panel is part of the chrome rather than a floating panel: the picture
is fitted beside it, so its width has to be known before egui lays
anything out. `chrome::Parts` says whether it and the transport bar are
up, `Chrome::new` gives it `filmstrip::WIDTH` down the left edge of the
window under the top bar, the left strip and the bottom bar starting at
its right edge, and `ui::show` derives the same `Parts` from whether it
was handed an `Input`, so the two cannot disagree — see
[the chrome](interface.md#the-chrome). Its head holds the two menus and the
back and forward pair and stays put; the rows scroll under it. The index
and name are laid over the thumbnail's corner on a wash of the bar's
ground rather than beside it, which is what keeps a row a fixed height and
the strip one thumbnail wide.

## The files seen

`app/visited.rs` is a browser's history: the files that have reached the
screen, in order, and where in that order the one on screen is. Going back
and then somewhere new — a step, a pick, a paste, an undo — cuts off what
lay ahead, so forward always leads to something reached from here. The
stack is told about arrivals rather than asking for them: `App::apply`
calls `Visited::arrived` for every fresh file, whatever asked for it, and
`Visited::back` and `forward` mark where they are heading so that the
arrival they asked for is a move along the stack rather than a new entry.
A file that has left the list is skipped rather than struck out, since
undo can put it back and it would be strange for it to have fallen out of
the past in the meantime; `listed` is `Files::position`, read at the
moment of the step. The pair at the strip's head and the `Alt` chords are
dead with nowhere to go, through `When::VisitedBefore` and
`When::VisitedAfter`, so the button, its tooltip and the press read one
`Conditions` as every other control does.

## A file taken off the list

`Backspace` takes the file on screen off the list and touches nothing on
disk. It leaves through the trash's door — `Files::hide` puts the path in
`hidden` and then `condemn`s it, and everything in
[editing](editing.md#the-list-keeps-a-trashed-file-until-something-replaces-it)
follows — because the reasons a deleted file stays on screen until its
neighbor is up apply to a removed one exactly. What is remembered is the
set: `Files::relist` leaves a hidden path out however often the directory
lists it, since the user has just said they did not want it there, and
`append` and `adopt` take a path back out of the set, since opening a file
by name is the user saying the opposite. Undo is `Edit::Removed`, which
`reinstate`s the file where it stood, and that takes it out of the set
too.
