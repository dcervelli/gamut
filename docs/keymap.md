# Keys and gestures

How a key pressed or a gesture made becomes something done, and how the
configuration file changes which. What each key and gesture does by default,
and how the file is written, are in [`user-docs/KEYS.md`](../user-docs/KEYS.md)
and [`user-docs/SETTINGS.md`](../user-docs/SETTINGS.md).

## Two systems pointing opposite ways

Keys and gestures share a file and nothing else. A key's line in the file is
a name followed by chords: `keys.zoom.in = + =`. A gesture's line is a slot
followed by one behavior: `gesture.image.middle.hold = loupe`. The two point
opposite ways because their cardinalities do. Many chords reasonably reach
one action — `+` and `=`, `]` and `Page Down` — so a name holds a list. A
slot on a surface is one place for the hand to be, and doing two things at
once there would be a conflict with no good answer, so a slot holds exactly
one behavior and writing it replaces what it held. The slots are a closed
set, which is why a gesture line never has to take anything from anywhere
else, where a key line does.

## Keys

`app/input.rs::ROWS` is the table: the lines `--help` prints, each with a
section, a help sentence, an optional `When`, and its `Keys`. A line's keys
are `Keys::Bound`, the names it binds — each a `keymap::Bound` with its
dotted name, its `Action` and its default chords — or `Keys::Also`, a name
bound on another line whose chords this line only describes (the region's
`Space` and `Ctrl+C` lines), or `Keys::Gesture`, a name's chords held while
dragging (`Space+Drag`). A name is its section's word, then what it does:
`zoom.in`, `files.undo`, `region.move.left`. The names are the file's
interface and so are stable; the help sentences are not.

`app/keymap.rs::Keymap` holds the chords in force, one list per name in the
table's order, generic over the rows so that its tests can use a small table
of their own. It keeps no record of which chords the user set, so a parsed
template equals the default and a round trip is an equality test.

### The chord

A `Chord` is modifiers and a `KeyName`: a character, a named key, or a place
on the keyboard. Shift is part of a character — `L` is the chord, not
`Shift+l`, and `>` is not `Shift+.` — because what Shift types depends on the
layout, and a table that asked for `Shift+.` would be asking for `>` on one
keyboard and `:` on another. So a character chord never carries Shift, and
the lookup takes Shift out of what is held before comparing. A named key and
a place on the number row are the same key whatever Shift does, so there
Shift is a modifier like the others: it is what tells `Shift+Left` from
`Left`, and `Shift+2` (50%) from `2`. The number row is the one part of the
keyboard bound by place, so that the zooms hang off the same keys on every
layout.

`Chord::read` and `Chord::token` are the file's spelling, and read back each
other; `Chord::spell` and `spell_all` are the people's, which fold four
arrows under one prefix into `Arrows` and a run of the number row into
`Shift+2, 3, 4`. The modifier words are `gestures.rs`'s, which both read.

A capital letter nothing binds is looked up as its lower case, so that Caps
Lock does not turn the keyboard off. The table therefore binds each letter in
one case, and a capital that is bound — `A`, `S`, `C`, `L`, `N` — is its own
key.

### Contexts

A chord has one holder in each context, and `Keymap::bind` enforces it:
binding a chord to a name takes it from whichever other name in the same
context held it, and returns the names it took from, which `Config::parse`
reports where an earlier line of the file had set them.

The only context is the region's (`keymap::Context::Region`), and it comes
from a line's `When`: `When::context` maps `RegionSelected` to it and every
other condition to nothing. The other conditions decide whether a key does
anything, not which key it is — `files.next` with one file does nothing, and
there is nothing else it should mean — so they only dim the help popup's
rows. The region is different: with one up the arrows could mean the region
or the view, and the user may want either. So dispatch tries the names of
each context that holds, in `keymap::ORDER`, before the plain names. By
default `region.move.*` sits on the same arrows as `pan.*` and wins while a
region is selected; moving it to another key leaves the arrows panning
under a region, and unbinding it leaves them panning with nothing moving the
region. An action whose behavior merely varies with the region — the fit's
five stops instead of three, the copy taking the region — is one name, and
`App::perform_on_region` decides what it does.

A new context is a `Context` variant appended to `ORDER`, and a `When` that
maps to it.

### Dispatch

`Keymap::action_for` takes winit's logical key, its physical key, what is
held, and whether a region is selected — the one reading of
`App::conditions` dispatch needs, taken directly rather than through
`Conditions`, which scans the visited files and asks the monitor. The number
row's place is tried first, then the named key or the character.

The fit key is answered on its way up — held, a drag zooms to a box — and
its release has to be found whatever is held by then, and whatever the
logical key reports. So `App::handle_key` keeps the physical key the press
arrived on in `Pointer::fit_key`, and a release of that physical key is the
fit; any key bound to `zoom.fit` works this way.

### The chooser's key

While the chooser's field has the keyboard, egui marks every key consumed
and the window never sees it. The key bound to `files.chooser` has to close
the chooser as it opens it, and reading `Ctrl+P` inside the pass would bind
it there for good. So `App::window_event` keeps a press whose action is
`OpenChooser` from egui while the chooser is open (and while no field has
the keyboard, so the press that opens it is not waiting in egui's input to
be typed into the field it focuses), and the press reaches `handle_key` like
any other. See [the chooser](chooser.md#who-gets-the-keys).

### Words that name keys

Everything that names a key — tooltips, the menus' shortcuts, the help
popup, the messages about undo and about the hidden interface — reads the
`Keymap` in force, so none of them can name a key that does something else.
`Namer` carries `Rc` handles on the application's keymap and gestures
rather than borrows: the namer is built as a value while the frame is drawn
with the application borrowed mutably, and an `Rc` clone is a pointer copy.
`--help` and the manual page are rendered from `Keymap::default()`: they
describe the program, not one user's file.

## Gestures

`gestures.rs` is at the top level, beneath both `ui/` and `app/`, because
the pass asks it which drag a button starts and the application which
wheel it turns. A `Slot` is a `Surface` and an `Input` — a button with its
modifiers and a `Kind` (drag, hold, click), or the wheel with its modifiers
and a button held. `Slot::read` and `Slot::token` are the file's spelling;
`Behavior::read` accepts only the words the slot's kind takes and names them
when it refuses one. A click's behavior is a key's name rather than a
vocabulary of its own: a click is a press, and would only mirror the keys'.
`Config::parse` checks the name against the keymap.

Lookups are exact: `Gestures::drag(surface, mods, button)` finds the slot
with exactly those modifiers, so `ctrl+left.drag` does nothing unless it has
a slot, which is how `Ctrl` with the wheel stays the compositor's by default.

On the picture, some of what the primary button does is fixed and comes
before its slot, in `Pass::region_gestures`: with the fit key held a drag
zooms to a box; with a region asked for it draws one; from a handle it pulls
the handle. These belong to the selection rather than to the hand, and a
file that could send a drag from a handle elsewhere would leave the handle
unreachable. After them the slot decides, for any button: `move-region`
takes hold of the region from inside it and, anywhere else, falls back to the
same button's slot held with nothing (`Pass::drag_action`), which is what
keeps `Shift` with a drag panning outside the region; `zoom-box` draws a box;
`pan` pans.

A button other than the primary held on the picture is reported every pass
as `Command::Held`, with the pointer, and `App::loupe_held` asks its hold
slot; the left button is a drag and cannot be a hold. A click comes back as
`Command::Click` only where its slot names a key and the button's hold does
nothing — letting go of a button that held the loupe is not also a click —
and `App::click` performs the key's action. The wheel comes back as
`Command::Wheel` with both deltas in notches and the held button; `App::wheel`
looks the slot up and zooms, pans, steps the loupe, or performs a key's
action once per whole notch, adding a trackpad's fractions up in
`Pointer::notches` until there is one. On the minimap a button whose drag
slot is `center` centers from the press on, and one whose only slot is a
click centers on the click.

A new behavior is a word in `WORDS` and a variant, and its arm in the pass
or in `App::wheel`; a new default is a line of `Gestures::default`.
