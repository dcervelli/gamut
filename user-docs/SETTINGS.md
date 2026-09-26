# Settings

`gamut` reads two files when it starts. One is yours and the other is its
own, and neither has to exist.

## The configuration file

`~/.config/gamut/config` (under `$XDG_CONFIG_HOME` if you set it) says how the
window opens every time. `gamut` only reads it: toggling a panel in the window
lasts until the window closes and does not change the file.

Each line is `name = value`, and `#` starts a comment. Anything left out keeps
its default. To start a file with every setting, every key and every mouse
gesture listed, commented out at its default, run:

```
mkdir -p ~/.config/gamut
gamut --print-config > ~/.config/gamut/config
```

Then remove the `#` from any line you want to change. A line you leave
commented out keeps following the default if a later version changes it. The
settings are:

| Setting | Default | What it does |
| --- | --- | --- |
| `show_ui` | `true` | The panels around the picture, which `` ` `` hides and shows |
| `show_minimap` | `true` | The minimap, while the picture is larger than the window |
| `show_filmstrip` | `true` | The file list, while there is more than one file |
| `show_histogram` | `false` | The histogram panel |
| `show_info` | `false` | The file information panel |
| `pixel_format` | `hex` | How the pixel under the pointer is read out: `hex`, `decimal` or `mapped` |
| `log_counts` | `false` | The histogram's bars as tall as the logarithm of their counts |

`--histogram`, `--info` and `--no-minimap` override the file for that one run.
If a line has a misspelled name or a value `gamut` does not recognize, it
names the line on the terminal and uses the rest of the file.

### Keys

Every key has a dotted name — `zoom.in`, `files.undo`, `region.move.left` —
listed beside it in [Keys and mouse](KEYS.md), and in the file
`--print-config` writes. A line binds the name to the keys after it,
separated by spaces, in place of its defaults:

```
keys.zoom.in = + =
keys.files.undo = ctrl+z
keys.files.export = ctrl+shift+e
keys.files.chooser =
```

The last line binds `files.chooser` to nothing, so no key opens the chooser,
and the help popup (`?`) marks its line **unbound**.

A key is written as the modifiers held with it, each followed by `+`, and
then the key itself. The modifiers are `ctrl` (or `control`), `alt`,
`shift` and `super`. The key is one of:

- a character, written as your keyboard types it: `>` rather than
  `shift+.`, and `L` for `Shift` with `l`. `shift+l` means the same as `L`;
  `shift` with any other character is refused, since what `Shift` types
  there depends on the layout. A trailing `+` is the key `+`, so `ctrl++` is
  `Ctrl` with `+`.
- a digit, `0` to `9`, which is the key on the number row wherever your
  layout puts the character: `shift+2` is `Shift` with that key, whatever it
  types.
- one of `space`, `enter` (or `return`), `esc` (or `escape`), `tab`,
  `backspace`, `delete` (or `del`), `insert`, `home`, `end`, `pageup`,
  `pagedown`, `left`, `right`, `up`, `down`, and `f1` to `f12`. These names
  are read in any case; a single character is not.

`#` starts a comment, so to use the key that types it, write the number-row
key it is on: `shift+3` on most layouts.

A key has one job at a time. Binding a key to a name takes it away from
whichever name had it, so a key moved from one job to another does not also
keep the old one; where an earlier line of the file had given it to the
other name, `gamut` says so on the terminal.

The region's names — `region.move.*`, `region.grow.*` and `region.shrink.*`
— are the one exception. While a region is selected they are tried first,
and any other key does what it does without one. By default they are on the
arrows, which is why the arrows move a selected region rather than panning.
Bind `region.move.left` to `h` and Left pans under a region again, while `h`
moves it; unbind `region.move.left` and Left pans under a region with
nothing moving it left.

Some keys are not in the table and cannot be rebound: `Esc` inside a popup
or dialog, `Enter` in the rename and export dialogs, and the arrows, `Page
Up`, `Page Down`, `Home`, `End` and `Enter` in the file chooser while you
type in it. The chooser's key only opens it; `Esc` closes it, and while it
is open its key types into it like any other. A long list of keys on one name
may wrap in the help popup.

### Gestures

Each thing the mouse can do on the picture or the minimap is a slot, named
`gesture.<surface>.<what is done>`, and holds one behavior:

```
gesture.image.middle.drag = pan
gesture.image.middle.hold = loupe
gesture.image.ctrl+wheel = exposure
gesture.image.left.drag = zoom-box
gesture.image.back.click = none
```

The surface is `image` or `minimap`. What is done is a button, held with any
modifiers, and how it is used: `left`, `middle`, `right`, `back` or
`forward`, then `.drag`, `.hold`, `.click` or `.double-click` —
`shift+left.drag`, `middle.click`, `left.double-click`. Or it is the wheel, turned with modifiers or with a button
other than the left held down: `wheel`, `ctrl+wheel`, `right+wheel`,
`ctrl+middle+wheel`. The left button cannot be a hold, being a drag.

What each takes:

| Gesture | Behaviors |
| --- | --- |
| A drag | `pan`, `zoom-box`, `move-region`, or `none` |
| A hold | `loupe`, or `none` |
| The wheel | `zoom`, `loupe-magnification`, `exposure`, `black-point`, `white-point`, `files`, `frames`, `pan`, or `none` |
| A click or a double-click | the name of a key, which it then does — `files.back`, `interface.grid`, `zoom.100` — or `none` |
| A drag or a click on the minimap | `center`, or `none` |

Each step of the wheel is a notch, and a trackpad's scroll adds up to a
notch before anything moves, except for `zoom` and `pan`, which follow a
trackpad smoothly; `pan` follows it both ways. Turning the wheel up raises
the exposure or the point it steps, and goes to the previous file or frame.
The first click of a double-click is a click as well, and does whatever
that button's click does. `move-region` moves a selected region when the drag starts inside it, and
elsewhere does whatever the same button does with nothing held.

A gesture is matched exactly: a slot for `left.drag` says nothing about
`ctrl+left.drag`, which does nothing unless it has a slot of its own. A
wheel with `Ctrl`, `Alt` or `Super` held does nothing by default, as those
belong to the window manager.

Some of what the left button does on the picture comes before its slot and
cannot be changed: a drag begun while a region is asked for draws it, one
from a handle pulls the handle, one with `zoom.fit`'s key held draws a box
to zoom to, and a click on a handle makes it the current one.

## The state file

`~/.local/state/gamut/state` (under `$XDG_STATE_HOME` if you set it) is where
`gamut` remembers settings you change by hand: how wide you dragged the file
list and how far the loupe magnifies. It is written when the window closes
and read when the next window opens. You never need to edit it, and deleting
it resets both to their defaults.
