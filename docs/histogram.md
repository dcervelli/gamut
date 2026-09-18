# The histogram panel

`src/ui/histogram.rs` draws the panel; `src/image/stats.rs` counts what it
plots; `src/image/display.rs` holds what its controls set. This page is about
the shape of the panel — what is on it, what is not, and why — rather than
about the plot's arithmetic, which the code explains beside itself. What the
controls do to the picture is [color management](color.md)'s page.

## Where the line is

`gamut` is a viewer. The question the panel exists to answer is *what does
this file hold* — is the shadow empty or merely dark, is the highlight gone
or merely bright, where in the file's range do its values actually sit — and
a control earns its place on the panel by helping answer it. Exposure does:
pushing a picture two stops up is how you find out whether a shadow has
anything in it. A black point and a white point do: they are how linear or
16-bit data becomes a picture at all. White balance, saturation and the like
do not. They answer no question about the file, they only make a different
picture, and a viewer that makes a different picture it cannot save is a tease
that ends in sidecar files. That work belongs to the programs on the open
menu. The panel is an instrument, not `Display`'s fields laid out as rows.

## The rows are every file's

Three rows of settings under the band — *Exposure*, *Window*, *Curve* —
and every file gets all three, so the panel is one height
(`histogram::SIZE`) for every file and for the whole of a file's stay. The
information column starts under the panel, and a column that jumped from one
file to the next, or every time a handle was dragged past white, would be a
column no one could read.

Two audiences use this program — `AutoWindow::default_for` splits on
`Referred`, and the colormaps, the float TIFF and the GeoTIFF keys say who
the second audience is — and a graded file's window is 0..1 by rights where
measured light is windowed to what it holds. The rows do not follow that
split, because the black handle moves the window on every file: a graded
file's window leaves 0..1 at the bottom the moment the black handle is
touched, so a white handle that meant the exposure on a graded file and the
window's top on a measured one would be a handle that meant a different
thing on the next file along. The two dials are honestly two. The window is
in the file's own units — both handles, both pairs of keys, the *Window*
row — and the exposure is in stops — the slider, `d`/`f`, `--exposure` — a
push on top of whatever the window is. On a graded file the *Window* row is
where a hand-moved window is put back, *As stored* being 0..1, as much as
where a rule is chosen.

The curve row is every file's because the exposure is: a stop up puts the
top of any file above white and the bar says **clipped**, and the curve is
the answer to that. A row offered only to a file that opens above white
would leave `t` putting a curve on a file whose panel had no row for it, and
the bottom bar naming a curve there was no button for. It is not the
false-color case, where the key is dead because the curve does nothing; on
an 8-bit file at 0 EV the neutral curve still takes its offset out of the
shadows, which is something, and the response curve on the plot shows it.
The row is *Curve*, and its two cells are *Clip* and *Roll off*, since what
the row chooses is what becomes of the light above white on a surface that
stops there, and a cell that named the curve would name a thing to look up
rather than a thing to see. There is one curve to roll off with —
[color.md](color.md) says why only one.

`histogram::SIZE` is what `ui::PANELS_ROOM` — the least window the
interface fits in — is measured against, and what `ui::room` and
`info::panel` work from, so that the toggles and the column agree with the
panel about how tall it is.

The three windows are named for what they do — *As stored*, *Full range*,
*Trimmed* — rather than for the rule that does it, which the tooltip says,
and the bottom bar says the same in one word each: *stored*, *full*,
*trimmed*, and *manual* once a handle has moved. There is no fourth button
for "the image's own", because the file's own is always one of the three —
*As stored* on a graded file, *Trimmed* on measured light — and the reset
button puts it back.

## The band is the levels track

The band under the plot is the axis — what the display makes of each value
along it — and the handles on it mark the window, which makes the band the
levels track every editor has: drag the black handle to set what comes out
black, the white handle to set what comes out white, the band between them
to slide the window along. Nobody has to be told what the two handles do,
where the same four operations spelled out as a reading and four chevrons
would have to be explained.

The handles stand at the *displayed* bounds — `Display::displayed_bounds`,
exposure folded in — because those are the values the band goes black and
white at, and a handle that stood anywhere else would mark a value the shader
is not clipping at. A handle is dragged *to* the pointer rather than *by* it —
`Command::BlackPoint` and `Command::WhitePoint` carry the value the hand asks
for, on every frame of the drag — so the interface holds no state about the
drag and a hand that runs off the end of the band puts the handle at the end.
The band's own drag, `Command::Slide`, is by the hand's movement, since what
it is sliding is wherever the window already was.

Each handle moves its own end of the window and nothing else:
`Display::put_black` and `Display::put_white`, twins. Exposure and the
window's top are two dials for one effect — a stop up is white moved to
half its value with black held — and they stay two dials, because they
answer differently to everything else: the window is found from the pixels
and found again when the file changes on disk or a *Window* rule is pressed,
and the exposure survives that as the push on top, which is what lets
`--exposure -1` mean the same thing over a directory of frames that each
keep their own trimmed window. `Display::set_displayed_bounds` works the
window back from the two displayed values with the exposure left as it is,
so a hand-set white under a stop of exposure is that white with the stop
still on top of it.

The slide stops at the plot's ends, as the handles do because the band
does. A window slid off what is plotted makes nothing black, or nothing
white: a lift, which is a grading operation and not a place to look. So the
band pans within the plot — *which part of the
range, at this width* — which is only a question once the window is
narrower than the plot: after some exposure on a graded file, or a trimmed
or hand-set window on data. A window as wide as the plot does not move,
since there is nowhere for it to go.

The keys are the handles' twins. `a`/`s` step the black handle and `A`/`S`
the white one, each by a twentieth of the window's width along the plot
(`input::WINDOW_STEP`) and no further than the plot goes, through
`Display::step_black` and `Display::step_white`. Along the plot, on the
file's own curve, and not through linear light: the plot gives the shadows
most of the band on a graded file, so a twentieth of the *light* is a
quarter of the band on the first press from 0 and a tenth on the next, the
handle bounding out of the shadows and then slowing. Stepped on the curve
it moves the same distance on the band every time, as a drag does. A step
that the plot's end stops short at is no step, and says so by returning
`false` so the key does not redraw; a press at the end asks for the end,
and the end can stand a rounding error off the handle, so a move of a hair
is no move either. The keys are in the handles' basis — an end each —
rather than window and level, so that each handle's tooltip can name a key
that does what the handle does. Nothing slides the window from the
keyboard, and the band's tooltip says so by naming no key. A narrowing is
two presses.

A window can still end past what is plotted, which a few stops of exposure
the other way is enough to do. Such a handle is drawn hollow at the edge it
went out of: it can still be taken hold of and brought back, and it does not
claim a boundary the curve running on past it says is not there.

The value under the hand is written on the line above the plot rather than in
a tooltip. egui takes a tooltip down for the length of a drag, and a drag is
exactly when the number is wanted.

## What is written in the corners

`Plot::clipped` is the share of the samples at or below the value that comes
out black and at or above the one that comes out white, per plane, the worst
plane reported. A red flower blows its red channel long before its luminance
goes, and the plane that has piled into the end bin is the one the eye finds
on the plot. The panel writes the two shares in the plot's top corners, in
the accent, over a backing of the plot's own ground so that the number stays a
number on a bar that has climbed into the corner. Only when there is a share
to write: an empty corner most of the time is what makes a number in it news.

The right corner is only written where the surface is actually clipping —
no curve, and an SDR surface, the same test the bottom bar's **clipped**
makes, or a false color, which clips whatever the curve. A curve rolls the
highlights off rather than throwing them away, and an HDR surface shows them;
neither is a share of the picture lost.

The share is exact on a quantized file, whose codes land one to a bin: a bound
sitting on a code counts that code and everything past it. On a float axis
the bins are wider than a code and the share is to the nearest bin.

## The marks on the picture

`w`, held, paints the clipped pixels on the picture: `MARK_WHITE` and
`MARK_BLACK` in `shaders/image.wgsl`, where every channel of the windowed
value is at or past the bound, painted in place of the pixel and before the
false color, whose ramp does not end in white and black. The whole test lives
in the image shader because that is the one place the windowed value exists
per pixel; the composite pass sees the image target, which for a
false-colored file already holds the ramp's color.

Held rather than toggled, and answered on the way down and taken back on the
way up as `Space` is: the marks are a thing to glance at, not a state to be
left in, and a toggle would need a button — which the strip down the left of
the plot has no room for without growing the plot. `Pointer::marking` is the
state, `shader_codes::marks` the two bits, and `Scene::mark_clipped` the way
in, rather than a field of `Display`: `Display` is kept per file and restored
when a file comes back, and a held key must not come back with it.

Which ends are marked follows `Display::clips_white`, the same rule the
corner and the bottom bar's **clipped** use: white is only marked where the
surface is actually clipping it. The paint marks a pixel where *every*
channel has gone, and the corners count a channel at a time; the two
therefore disagree on a red flower, and are meant to — the corner is the
histogram's own reading, plane by plane, and the paint is where the picture
has gone flat.

## What is drawn only when it says something

The response curve is the whole of what the display does, and on a display
with nothing asked of it — `Display::is_identity` — it is the diagonal from
corner to corner, which says nothing the axis under it does not. It is drawn
only once the display is doing something, so that a line across the plot is
news rather than furniture.

The value under the pointer is written only while the pointer is over the
plot. The rule that marks a bin still follows the pointer over the picture as
well, since where a pixel falls on the plot is worth pointing at, but the
pixel's own numbers are the bottom bar's readout, and writing them twice was
one readout too many.

The ends of the axis are written only where they are not 0 and 1 decoded — a
graded file's plot runs from black to white, which every histogram of such a
file does and no one needs told — and, where they are written, without
the zeros they do not need: a linear file's axis reads `0` and `3.984`, or in
counts where the file stores them.

The channel planes are drawn in a red, a green and a blue held short of the
primaries (`theme::HISTOGRAM_PLANES`). Screened over one another on the
plot's ground they still give a yellow, a cyan and a magenta where two overlap
and a near white where all three do, which is the reading a channel histogram
is looked at for; the full primaries, at a pixel to the bin, come out as a
hedge of pure red, green and blue spikes that the eye cannot leave alone.
The ground is the same in every theme and so are the inks, so the reading is
the same everywhere — see [theme](theme.md) for the two things that resist
being themed. A bin is a logical pixel, which fixes the panel's width and is
why nothing here smooths the plot.

## What goes dead

The row of curves is dead under a false color, and says why when rested on.
The compositor holds the curve at a clip there — `Display::false_colored`
is the test, and `composite.rs`, the bar's words and the pointer's readout
all make the same one — because a ramp has no color past its end for a
highlight to roll off into, and a curve over the ramp would bend the very
mapping the reading is being taken off. `Display::response` runs the curve
the compositor runs, the buttons refuse the press and `t` does nothing, and
the row stays where it is — the panel's height is the file's — rather than
leaving. A panel that lit whichever curve was chosen and let the buttons
change it would have a press change nothing on screen and then change the
picture some time later, when the ramp came off.

## The exposure is a slider

A spinner — a number with a step either side of it — is the right control
for a value that is mostly typed and occasionally nudged; the exposure is
neither. It is swept — up until the shadow shows something,
back until the highlight stops clipping — and a sweep wants a line with a
handle on it, where a length is the push and its direction is which way.
So the row is a slider: a groove with a mark at nothing, the run from there
to the handle filled in the accent, and the handle drawn as the band's are
because it is the same kind of thing. The reading stays, at the end of the
row, since a slider with no number is a slider you have to read the picture
to check.

It runs six stops each way (`SLIDER_STOPS`), not the sixteen the keys and
`--exposure` go to. A step is a quarter of a stop, and across the room the
row has that is a few pixels each at six, which is a slider that can be
put on a value, and under two at sixteen, which is not; and six is the
whole of what a viewer does with an exposure, a shadow lifted out of black
or a highlight brought back from four stops over. Past the end the handle
stands hollow at it, as a band handle out past the plot does, and the
reading says where the exposure really is. The slider is dragged *to* the
pointer, as the handles are, and asks through `Command::Exposure` only for
an exposure that is not already the one in force, so that a hand resting
on it is not a redraw a frame. It snaps to the quarter stops, so the
reading beside it is always one the keys could have reached, and so that
the keys and the slider cannot take the picture to two different places
that read the same.
