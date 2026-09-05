# Theme

The interface takes its colors from the desktop rather than carrying its own.
On [Omarchy](https://omarchy.org) the active theme is materialized as a
palette file, and `src/theme/` reads it, resolves it, and derives the
handful of roles the chrome actually needs — panel, hairline, primary and dim
text, accent, the menu panel, the ground the panels that float over the image
are read on. Switching the desktop's theme is picked up on the same
250 ms poll as the file on screen, so an open window changes with everything
else rather than staying in the theme it opened under.

Off Omarchy there is nothing to read and nothing happens: the neutral dark set
the interface was designed in is used instead. The same set fills in for a
palette too sparse to build on, so a half-written theme degrades to something
wearable rather than to black on black.

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

* **The histogram's plot is drawn on a near-black ground, in the primaries
  themselves.** The plot is drawn by screening the color planes over one
  another, and screening only reads on a dark ground: what is under the plot
  is added to every plane, so a ground light enough to see lifts each plane's
  darkest channel several times over and three overlapping planes come out as
  three washes of the same pale color. Pure red, green and blue on a
  near-black are the one set that behaves: two planes overlapping give the
  secondary between them and all three give white, which is the reading a
  channel histogram is looked at for — and is the same reading in every
  theme, which a plot made of a palette's own pastels is not.

  Themed planes were tried: each pulled towards its own primary and then
  scaled, whole, until the three screened together landed on a neutral mid
  gray. It works, in the sense that no theme blows the plot out — but the
  overlaps come out muddier the further a palette sits from the primaries, so
  how much a histogram can be read depends on the desktop's taste in reds.
  That is the wrong thing to make themeable.

  The panel *around* the plot is not one of these. Nothing is screened onto
  it, so it is the bars' own surface, mildly transparent, with the same ink on
  it as the bars carry — the same panel the file's information is read on, and
  the same one a popup's cells sit on, that last held nearer to opaque since
  the picture coming through a menu is what the choices on it compete with.
* **The luminance plane is a neutral gray.** It stands for a pixel's value
  rather than for one of its channels, so a hue on it would read as a fourth
  color.

