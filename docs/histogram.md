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
menu.

The panel had drifted from that line: it laid `Display`'s fields out as rows —
the window as two numbers with four nudges, the automatic rules as a row of
buttons, the curves as another — so that it read as the struct rather than as
an instrument. What follows is how it was brought back.

## The rows follow the file

`histogram::Offered` says which rows of settings a file gets under the band,
and it is settled from the file alone: `Referred::Scene` for the window row,
`Display::opens_above_white` for the curves. Every file gets the exposure.

Two audiences use this program, and the code already knew it —
`AutoWindow::default_for` splits on `Referred`, and the colormaps, the float
TIFF and the GeoTIFF keys say who the second audience is. The technical
controls are the right controls for measurement data and noise under a JPEG.
A photograph's window is 0..1 and nothing else, so a row that offers it two
ways to be wrong is a row the panel has to explain; a curve exists to fit
values above white into a surface that stops there, so under a file that
never reaches white it is a bend for no reason. Linear data has no white of
its own, and the three rules for finding one are the three buttons; a file
with headroom in it, graded or measured, gets the curves.

The rows are decided from the file rather than from what has been done to it
so that the panel is one height for the whole of a file's stay. The
information column starts under the panel, and a column that jumped every
time a handle was dragged past white would be a column no one could read
while dragging. The cost is that a photograph pushed past white by hand gets
no curve row; `t` still puts a curve on it, and the corner of the plot says
how much is being clipped, which for a viewer is the more useful fact.

`Offered::ALL` is the tallest the panel gets, and is what
`ui::PANELS_ROOM` — the least window the interface fits in — is measured
against. `ui::room` and `info::panel` take the file, or the tallest where
there is none, so that the toggles and the column agree with the panel about
how tall it is.

The three windows are named for what they do — *As stored*, *Full range*,
*Trimmed* — rather than for the rule that does it, which the tooltip says.
There is no fourth button for "the image's own": the row is only offered to a
scene-referred file, whose own window is the trimmed one, so it would be the
third button twice.

## The band is the levels track

The band under the plot always was the axis — what the display makes of each
value along it — and the ticks on it always marked the window. Now the ticks
are handles, and the band is the levels track every editor has: drag the
black handle to set what comes out black, the white handle to set what comes
out white, the band between them to slide the window along. That is what
replaced the `0.000–1.000` reading and the four nudges, which were the same
four operations spelled out as chevrons.

The handles stand at the *displayed* bounds — `Display::displayed_bounds`,
exposure folded in — because those are the values the band goes black and
white at, and a handle that stood anywhere else would mark a value the shader
is not clipping at. A handle is dragged *to* the pointer rather than *by* it —
`Command::BlackPoint` and `Command::WhitePoint` carry the value the hand asks
for, on every frame of the drag — so the interface holds no state about the
drag and a hand that runs off the end of the band puts the handle at the end.
The band's own drag, `Command::Slide`, is by the hand's movement, since what
it is sliding is wherever the window already was.

What moves to put white where the hand asks is the file's to say, and
`Display::put_white` asks it. Exposure and the window's top are two dials for
one effect: a stop up is white moved to half its value with black held. On a
photograph the window is 0..1 and nothing else, so the white handle there is
the exposure — the stops that land white under the pointer, snapped to the
quarter stops the buttons and the keys count in, so that the exposure row
reads as it would after so many presses and the handle reaches exactly the
numbers they do. On measured light the two genuinely differ: the window is
found from the pixels and found again when the file changes on disk or a
Window rule is pressed, and the exposure survives that as the push on top,
which is what lets `--exposure -1` mean the same thing over a directory of
frames that each keep their own trimmed window. So there the white handle is
the window's top, as the black handle is its bottom everywhere, and
`Display::set_displayed_bounds` works the window back from the two with the
exposure left as it is.

The slide stops at the plot's ends, as the handles do because the band
does. A window slid off what is plotted makes nothing black, or nothing
white: a lift, which is a grading operation and not a place to look, and on
a photograph it would move `high` off 1, which the white handle there is
careful never to do. So the band pans within the plot — *which part of the
range, at this width* — which is only a question once the window is
narrower than the plot: after some exposure on a photograph, or a trimmed or
hand-set window on data. A window as wide as the plot does not move, since
there is nowhere for it to go.

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
photograph's plot runs from black to white, which every histogram of a
photograph does and no one needs told — and, where they are written, without
the zeros they do not need: a linear file's axis reads `0` and `3.984`, or in
counts where the file stores them.

The channel planes are drawn in a red, a green and a blue held short of the
primaries (`theme::HISTOGRAM_PLANES`). Screened over one another on the
plot's ground they still give a yellow, a cyan and a magenta where two overlap
and a near white where all three do, which is the reading a channel histogram
is looked at for; the full primaries, at a pixel to the bin, came out as a
hedge of pure red, green and blue spikes that the eye could not leave alone.
The ground is the same in every theme and so are the inks, so the reading is
the same everywhere — see [theme](theme.md) for the two things that resist
being themed.

## What goes dead

The row of curves is dead under a false color, and says why when rested on.
The compositor holds the curve at a clip there — `Display::false_colored`
is the test, and `composite.rs`, the bar's words and the pointer's readout
all make the same one — because a ramp has no color past its end for a
highlight to roll off into, and a curve over the ramp would bend the very
mapping the reading is being taken off. The panel used to know none of
this: it lit whichever curve was chosen, drew it over the plot, and let
the buttons and `t` change it, so that a press changed nothing on screen
and then changed the picture some time later, when the ramp came off. Now
`Display::response` runs the curve the compositor runs, the buttons refuse
the press and `t` does nothing, and the row stays where it is — the
panel's height is the file's — rather than leaving.

## What stayed

The strip down the left — the two plane toggles, the count axis and the
reset — and the row of false colors under the band on a gray image are as
they were; so is a bin to the logical pixel, which fixes the panel's width and
is why nothing here smooths the plot. The exposure is still stepped in quarter
stops by two buttons, and the number between them can be dragged by the same
quarters, pressed through the same two controls so that a drag and a keystroke
cannot be worth different amounts.
