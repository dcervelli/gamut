# The file chooser

`Ctrl+P` opens a popup over the top of the picture: a field to type in, and
under it every file of the session that fits what was typed, each row with a
thumbnail, the name, what kind of file it is, its place in the list and its
size. `Enter` or a click opens the row under the cursor; `Esc`, a click
outside, or `Ctrl+P` again closes it. What a user does with it is in
[`user-docs/KEYS.md`](../user-docs/KEYS.md); this page is how it is built,
and why it is built that way.

Three files: `src/ui/chooser.rs` draws the popup and reads what was pressed
in it, `src/app/chooser.rs` holds everything the popup is drawn from, and
`src/thumbnailer.rs` is the thread that makes the thumbnails, writing them
into the desktop's own cache through `src/thumbnail.rs`.

## A fifth popup

The chooser is an egui popup like the four menus in
[the interface](interface.md#popups-and-menus), and its open state lives
where theirs does: in egui's memory, under the id `ui::chooser::id()`.
`App::press(Control::Chooser)` opens it with `Popup::open_id` — after
`Popup::close_all`, which is how one popup at a time is kept — and closes it
with `close_all`, exactly as `App::close_menus` closes a menu. Everything
that follows from that comes for free: `Esc` closes it, a click outside
closes it, and `q` or `Esc` pressed with it up close it rather than quitting,
since the key that puts things away already asks egui first.

What is not egui's is the list. The query, the rows that fit it, which row
the cursor is on and what is known about each file are the application's,
in `app::chooser::Chooser`, and are handed to the pass on every frame the
popup is open as a `ui::chooser::Input`. Every frame, and not only when
something changed: a memory-backed popup that is not shown for one frame is
one egui discards, so `App::redraw` asks `Popup::is_id_open` and builds the
input whenever the answer is yes. The rows themselves are an `Arc<[Row]>`
rebuilt only when the matches, the facts or the thumbnails have changed, so
a frame over a session of thousands of files clones a pointer rather than
the list.

## Who gets the keys

While the field has the keyboard, no key reaches the window. `egui-winit`
marks every key event consumed whenever any egui widget has focus, and
`App::window_event` drops a consumed key before `handle_key` sees it — which
is what stops `q` typed into the field from quitting, and is also why the
chooser's own keys cannot be read from the key table. They are read inside
the pass instead, at the top of `ui::chooser::show` before the field is
added, with `InputState::consume_key`, and come back as commands: `Ctrl+P`
as `Command::Press(Control::Chooser)`, `Enter` as `Press(Control::Choose(cursor))`
for the row the frame was drawn with, the arrows, `Page Up`, `Page Down`,
`Home` and `End` as `Command::Cursor(Step)`. `Esc` is left alone: egui's
popup closes itself on it. `Ctrl+P` therefore reaches `App::press` by two
routes — the key table while the popup is closed, and the popup while it is
open — and both toggle the same control, so the two cannot drift.

One frame needs care. egui is handed every key whether or not it wants it,
so the chord that opened the popup is still in egui's input on the first
frame the popup is drawn, and read there it would close what it had just
opened — which is exactly what happened before `Input::opened` existed. On
that frame the chord is consumed, so it does not reach the field as text,
but not acted on.

The field asks for focus whenever nothing has it: on the first frame, and
again after a click on the popup's own frame takes it away. When the popup
closes, the field is not laid out on the next frame, and egui's dead-man's
switch drops the focus of a widget that has disappeared. That frame is what
hands the keys back to winit's table.

The field is drawn by hand around a plain `TextEdit` — a box in
`bar_background` with a `border` hairline — because egui's own ground for a
text field is `extreme_bg_color`, which this theme sets to the hairline
color for the scrollbar's sake, and a field in it would read as a rule
rather than a box. Arrowing through the list opens nothing: `Enter` does,
so a row can be looked at without being loaded.

## Matching

The query is matched against each file's path relative to the deepest
directory every file in the session shares — `app::chooser::common_dir` —
so that a session over one directory matches on names alone, and a session
over several can be narrowed by where a file is. A row shows the name in
bold and, only when the session spans more than one directory, the relative
directory dim beside it. The matcher hands back the char indices it found
the query at in `dir/name`, and the row splits them at the separator by the
directory's char count, so a hit in a name with a multi-byte character
before it lands on the right glyph.

The matcher is behind a trait, `fuzzy::Matcher`, whose one method is skim's
own `FuzzyMatcher::fuzzy_indices` signature for signature. The
implementation is `fuzzy-matcher`, which is skim's algorithm as it was cut
out of that program in 2020 and looks unmaintained since; skim's own tree
has kept the same code moving under `src/fuzzy_matcher/`, and skim itself is
the whole terminal finder — tokio, ratatui, crossterm — rather than a
library. Should the crate ever need replacing, a vendored copy of skim's
current `skim.rs` and `util.rs` (MIT, with a `[[annotations]]` entry in
`REUSE.toml`) is a second `impl Matcher` in `src/fuzzy.rs` and nothing else
moves. `nucleo`, the other candidate, is MPL, which `about.toml` does not
accept. `src/fuzzy.rs` is the only file that names the crate, and the
chooser's tests run over a matcher of their own — `Plain`, a leftmost
case-blind subsequence — so they state what the chooser needs of any
matcher rather than what one library scores.

`rank` sorts by score with a stable sort, so ties keep the list's order,
and an empty query is the whole list in order with nothing lit. A query
beginning with `:` does not go to the matcher at all: `rank_by_index` puts
the file at exactly that place first and every place with those digits in
it after, in the list's order, which is what the index column is there
for.

## The cache

Thumbnails go into the freedesktop thumbnail cache — `~/.cache/thumbnails`,
or `$XDG_CACHE_HOME/thumbnails` — in the `x-large` directory at 512 pixels a
side, which is what GNOME's own files there are. The cache is the desktop's
rather than this program's on purpose: a thumbnail made here is one the file
manager finds, and one the file manager made is one this program finds
without decoding anything, which for a directory that has ever been opened
in one is most of them.

That only works if the key agrees to the byte. A thumbnail is named by the
MD5 of the file's URI, and the URI has to be spelled as GLib's
`g_filename_to_uri` spells it, since GLib is what every other writer of the
cache goes through: `A-Za-z0-9` and `!$&'()*+,-./:=@_~` left alone, every
other byte percent-encoded in upper-case hex. `thumbnail::uri` is that
spelling, checked against a run of GLib itself; `clipboard::file_uri`
escapes more than GLib does — correctly, for a URI, and uselessly for a
key — and must not be used here. MD5 is in the tree, eighty lines against
RFC 1321's vectors, rather than a crate that brings five others for one
function.

A thumbnail counts only when its `Thumb::URI` and `Thumb::MTime` chunks name
this file at its current modification time; anything else is missing, and
is made again. What is written carries those two, `Thumb::Size`,
`Thumb::Image::Width` and `Thumb::Image::Height`, and `Software`, all ASCII
in `tEXt` chunks ahead of the pixels, where a reader of the header alone
finds them. A file this version cannot thumbnail is recorded as a one-pixel
PNG under `fail/gamut-<version>`, so it is not tried again on every visit
and a new version, which may read it, tries afresh. Directories are made
`0700` and files `0600`, and each file is written to a temporary name in
the same directory and renamed into place: nothing reading the cache can
open half a thumbnail, and the process leaving mid-write leaves nothing
under a thumbnail's name. A file inside the cache is never thumbnailed.

## The worker

One thread, `gamut thumbnailer`, started at start-up over the whole session
so that the cache fills while the first file is being looked at. It lowers
its own priority first — nice 10, and the best-effort I/O class at its
lowest level, through `libc` since the standard library has no way to say
either — because the thumbnails are for a list the user may never open and
the loader thread is decoding the picture they are waiting on. Both are
per-thread on Linux, which is what makes lowering them here rather than for
the process worth doing. One thing has to happen before the drop: rayon's
global pool is built, by `rayon_core::ThreadPoolBuilder::build_global`.
The pool is made lazily by whichever thread first uses it, and its threads
inherit that thread's nice value for the life of the process, so left to
chance a JPEG XL thumbnailed here before the loader decoded one would have
left every JPEG XL the loader decoded afterwards running at nice 10 — the
opposite of what lowering the priority was for. `top -H` shows the pool as
`gamut rayon N` at nice 0 and the thumbnailer at nice 10.

Per file: the header first, and its facts — the size and whether the file
holds frames or pages — delivered at once, so the row fills in before any
pixel work; then the cache; then, if the cache has nothing, the decode. A
file whose header claims more than `MAX_THUMBNAIL_DECODE_BYTES` would hold
at four channels of floats is refused before it is read and recorded as a
failure — the same gigabyte the [player](animation.md) keeps its cache
under, being the same question of what a background thread may hold. The
decoded image is box-filtered to 512 pixels by `image::resample::downscale`,
in the file's own encoding and with its no-data sentinel left out of every
mean, and the full image is dropped before anything else is done. The small
image is then scanned and windowed as the viewer would open it —
`Display::for_image_with` with the default start-up state — because
`Display::default` shows a scene-referred EXR or an elevation model as
black, and on a quarter of a megapixel the scan and the walk are cheap.
`encode::displayed_on` runs the display pipeline over it on one band, so the
thread stays on one core; `encode::png_with_text` writes it with the chunks.
A copy at 128 pixels, at most 64 KiB of RGBA, is what goes to the screen.

The queue is a deque plus the set of every path that has been in it: an
enqueue skips what has been seen, a prioritize moves — or puts back — its
paths at the front in the order asked. The chooser prioritizes the rows on
its screen whenever they change (`Command::Visible`), and the first page of
rows as it opens; a file changed on disk is prioritized as it is re-read,
since its thumbnail's modification time no longer matches. Replies arrive
as `UserEvent::Thumbnail` through the loop's proxy, like the loader's, and a
redraw is asked for only while the popup is open; with it closed the news
is taken in and no frame is drawn.

The thread is told to stop and not joined, which is the opposite of the
loader's decision and for the opposite reasons: it holds no handle on the
GPU device, a decode part way through would hold the quit up for a picture
nobody asked to see, and the temporary-then-rename write means leaving
mid-write leaves nothing behind.

## What the screen holds

Thumbnails on screen are egui textures, made with `Context::load_texture`
from the delivered RGBA; `Renderer::render` already applies egui's texture
deltas, so no GPU code knew about them. `app::chooser::Thumbs` keeps at most
`MAX_THUMBS` of them, about 32 MiB, and lets the least recently seen go —
seen meaning on the popup's screen, which is what `Command::Visible` touches.
An evicted thumbnail is asked for again when its row is next on screen, and
comes back from the cache rather than from a decode.
