# Theme

The interface takes its colors from the desktop rather than carrying its own.
On [Omarchy](https://omarchy.org) the active theme is materialized as a
palette file, and `src/theme/` reads it, resolves it, and derives the
handful of roles the chrome actually needs — panel, hairline, primary and dim
text, accent, and the menu panel. Switching the desktop's theme is picked up on the same
250 ms poll as the file on screen, so an open window changes with everything
else rather than staying in the theme it opened under.

Off Omarchy there is nothing to read and nothing happens: the neutral dark set
the interface was designed in is used instead. The same set fills in for a
palette too sparse to build on, so a half-written theme degrades to something
wearable rather than to black on black.

`Theme` stays the source of truth, and what egui draws is derived from it in
`src/ui/style.rs`: the bars' color is its panel fill, the menu's its window
fill, the hairline its window stroke, the accent its selection and what a
lit button is washed with, the two text inks the strokes of its interactive
and quiet widgets, the theme's yellow and red its warning and error inks. Its
shadows are off, its corners are the panel's and a toggle's, and its text
styles are the interface's one size. Everything drawn by hand — a toggle, a
plot, a swatch — reads its inks from `Theme` directly rather than back out of
egui's style, so the two cannot come to disagree about a color. A change of
palette is put on egui's context the same tick it is read.

The faces follow the same rule, in `src/ui/fonts/`: the interface is set in
whatever the desktop calls `sans-serif` and `monospace`, with the bold sans
for the one bold thing in the window, and ships no font of its own. Which
face that is, is asked of fontconfig's own library — the answer `fc-match`
prints — rather than worked out from its configuration files. `fontdb` can
read those files itself, but it honors only the `<alias>` elements, ignores
the `<match>` rules Omarchy uses to name its faces, and lets the last alias
in file order win; on an Arch desktop that lands on Nimbus Sans Narrow, a
condensed face nothing else on the desktop is set in. The library is opened
at run time, so a machine without it still gets a window, set from `fontdb`'s
rougher reading. A face fontconfig
answers with is checked against the question, since it always answers with
its nearest: a bold that came back regular, or a monospace that came back
proportional, is treated as no answer, and that family falls back to the
sans.

On a Mac the faces are AppKit's: the system font, its bold and the system
monospace. San Francisco is one variable file, so each face is handed to egui
with the weight to set on its axis; see [macOS](macos.md#what-differs-and-why).
The colors are AppKit's too; see below.

Each face is handed to egui with one number worked out from its own
metrics. egui makes a row as tall as ascent, descent and line gap together,
puts the baseline the ascent down from the top, and centers that box in a
bar or a button; where the letters sit inside the box is the face's
business, and faces differ. Nimbus Sans Narrow declares its ascent no
higher than its capitals and a fifth of an em of line gap, all of it under
the baseline, so its text would ride a quarter of an em high in every button;
Liberation Sans and Adwaita Sans are centered to within a pixel. The
number is the distance from the box's middle to the capitals' middle, set
as the face's `y_offset_factor`, so the capitals sit at the middle of the
row whatever face the desktop supplies. It is read with `skrifa`, the
reader egui's own layout uses, so the two see the same ascent and descent.

On a Mac there is no palette file, and the appearance stands in for one.
`src/theme/macos.rs` makes the application's effective appearance the
current drawing appearance and reads AppKit's semantic colors under it —
`windowBackgroundColor`, `labelColor`, `secondaryLabelColor`, `textColor`,
`separatorColor`, `controlAccentColor` and the system red, yellow, orange
and blue — into a `System`, and `Theme::from_system` derives the same roles
from it that `Theme::from_palette` derives from a palette. The window's
background is the bars', the two label colors the primary and dim text,
`textColor` the file's name, the separator the hairline, and the user's
accent what is switched on. Reading them under the appearance rather than
from a table is what makes them faithful: the raised-contrast appearances,
and whatever a later macOS does to its grays, come with them unasked.

The label and separator colors are translucent, meant to be drawn over what
is under them, so each is laid over the window's background to find the
opaque ink it comes to there: the interface's text inks are opaque, and the
hairline is also the second square of the checkerboard. Two choices are the
Mac's rather than AppKit's colors read straight: a caution is written in the
system yellow on a dark window and the system orange on a light one, where
yellow all but vanishes; and the graphite accent, being a gray, lights
nothing among gray text in the chooser, which lights its hits in the system
blue instead.

There is no file to watch for a change of appearance or of accent, and
asking AppKit again is a handful of lookups, so on a Mac the theme is
simply read again on every poll and compared: the interface is retinted
only when the answer differs. The appearance is the main thread's to ask,
and the poll runs there; asked from anywhere else — which is every test —
the read answers nothing and the neutral dark set is used.

The palette is not read literally. Omarchy resolves it through an alias and
derivation cascade before any consumer sees it — short names, ANSI `color0`
through `color15` in both directions, shades mixed out of the base colors —
and a theme is free to define only one side of any of those pairs.
`src/theme/palette.rs` reimplements that cascade rather than shelling out to
`omarchy-theme-color`, which would cost a process per read and is not there to
be called on a machine that has no Omarchy on it. Its tests check the result
against what that script prints for the same file, so the two cannot drift
apart quietly.

One role is derived rather than read: **the ink the file's name is written
in.** It is the theme's `bright_foreground`, but only where a theme has
actually parted that from its ordinary `foreground` — several define the two
identically, which would leave the name reading exactly like the facts it
shares the bar with, and the name is the one thing in the window that says
what is being looked at. Where they collapse, the name's ink is carried away
from the page instead: towards white on a dark theme, towards black on a
light one, "bright" meaning further from the ground than the ordinary text
rather than lighter in itself. It is the same shape of fallback the hairline
gets when `lighter_background` resolves back to the background it sits on.

Two more resist being themed and are not:

* **The histogram's plot is drawn on a near-black ground, in one red, one
  green and one blue that no theme chose.** The plot is drawn by screening
  the color planes over one another, and screening only reads on a dark
  ground: what is under the plot is added to every plane, so a ground light
  enough to see lifts each plane's darkest channel several times over and
  three overlapping planes come out as three washes of the same pale color.
  A red, green and blue that each lead in their own channel, on a
  near-black, are the set that behaves: two planes overlapping give the
  secondary between them and all three give a near white, which is the
  reading a channel histogram is looked at for — and is the same reading in
  every theme, which a plot made of a palette's own pastels is not: the
  overlaps come out muddier the further a palette sits from the primaries,
  and how much a histogram can be read is the wrong thing to hang on the
  desktop's taste in reds. They are held a way short of the pure primaries,
  which at a pixel to the bin come out as a hedge of spikes the eye cannot
  leave alone; see [the histogram panel](histogram.md).

  The screening itself is done on the CPU, one column of the plot at a time.
  egui has one blend, so the planes cannot be laid over one another and left
  to the GPU; instead each column — one to a bin, a logical pixel wide — is
  cut into stretches at the heights of the planes standing in it, and each
  stretch is filled with what the planes over it come to, screened in code,
  which on pure primaries over near-black is the same picture a GPU blend
  would give.

  The panel *around* the plot is not one of these. Nothing is screened onto
  it, so it is the bars' own surface, with the same ink on it as the bars
  carry — the side panel the file's information is read on as well, and the
  surface a popup's cells sit on, that last just short of opaque.
* **The luminance plane is a neutral gray.** It stands for a pixel's value
  rather than for one of its channels, so a hue on it would read as a fourth
  color.

