# The histogram panel

`src/ui/histogram/` draws the panel; `src/image/stats.rs` counts what it
plots; `src/image/display/` holds what its controls set. This page is about
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

## A column of stages

The panel is the picture's way from the file to the screen, a section to a
stage, read from the screen back: **Output**, what goes out; **Display**,
what the display starts from and what it does to it; **Gain map**, how far
the map lifts the picture, for a picture that carries one; **File**, the
picture as the file stores it. `histogram::Section` is that list, in that
order, and `section::layout` lays out the ones the picture passes through
(`Section::applies`).

Output is at the top because what goes out is what the panel is most often
opened to ask about — is anything clipped, and how much — and the head of
the column is the part that is always in view. Below it the order is the
data's, so the column reads as one path rather than as four histograms that
happen to share a panel: a picture's red that is past white on the Display
plot is at the top of the File plot under it, and the matrix that moved it
is the gap between the two. A layout that kept every section in one place
for every file would have to leave a hole where a picture has no gain map;
the column closes up instead, and the sections above the gap do not move.

Every section is a fixed height (`Section::height`) for every file, and the
column is as tall as its sections, whatever the window. On a window shorter
than that the side panel scrolls — an egui `ScrollArea`, the wheel's alone:
`DragScroll::Never`, since a drag in the column is a drag of a handle, the
band or the exposure's slider, and must never move the column instead.
Nothing is shrunk to fit and nothing is dropped. What the panel needs room
for at least is the head section (`histogram::MIN_HEIGHT`), which is what
`ui::PANELS_ROOM` and `ui::room` are measured against, so that the toggle is
alive wherever the question the panel is opened for can be answered; the
rest is a scroll away. The width is the [side panel's](interface.md#the-side-panel).

The panel is laid out as the [information panel](interface.md#the-side-panel)
is, so that the two read as two tabs of one panel: a row of buttons at its
head, as tall as the information's header (`info::CHIP_HEIGHT`), so the
hairline under it (`info::HEADER_GAP`) stays put as the tabs are switched;
then the column, in a scroll area with its bar down the inner edge and the
gutter kept clear for it; and the sections parted by hairlines through the
gap between them (`info::SECTION_GAP`). Each section opens with a header
as the information's sections do: its mark and name in the accent
(`Section::mark`) — a monitor for Output, sliders for Display, the sun for
the gain map, as it wears everywhere else in the window, and a sheet with a
picture for File — and on the right what the pointer is reading at its
stage. The header's last square (`section::button`) is the section's own
button where it has one: the Gain map section's eye, which shows the map
in the picture's place, lit while it is. It is kept clear in every other
section, so the readouts end on one line down the column, and it is where
the switch that passes a stage by will go.
A stage added later — the development stages a ProRAW file describes, a
baseline exposure, a profile's tone curve, a gain table — is a `Section`
variant placed between Display and File at a height of its own: a 1D stage
composes into the Display curve and takes a header of its own, and a stage
that lifts each pixel by its own amount is shaped as the Gain map section
is, a histogram of the lift and a band of how much of it is applied.

The row of buttons — the luminance and color planes and the logarithmic
counts, then, set apart, the marks on the picture, and at the far end the
reset (`histogram::toolbar`) — acts on every plot in the column, or on the
picture, and on no one section, so it stands above all of them and stays
in view however far the column is scrolled. With nothing beside them, the
plots fill the column's width, which is a bin to the point:
`histogram::COLUMN_WIDTH` is the plot and the inset its ground is drawn
into, and the side panel's width is worked out from it.

## What each section plots

**File** is `Face::stored`: `Stats::scan_as_stored`, the picture read with
the curve resolved and nothing else — no lift, and the file's own primaries
(`Reader::as_stored`). It is measured once, when the image arrives, on the
loader's or the player's thread beside the scan the display starts from; a
file whose reading moves no color — gray, or BT.709 with no gain map
(`DecodedImage::color_moves`) — is not walked twice, and the stats are
shared.

**Gain map** is `GainMap::lift_plot`: each of the map's values at the whole
of the lift, in stops, over the stops the map holds. A map with a channel
each has a plane each, and its luminance plane is the mean of the three in
stops, as a pixel's luminance is a mean of its channels. Under it the band
is filled in the accent as far as the lift is applied
(`GainMap::applied_stops`, the same number the information panel's *Applied*
row says), which is none on a screen with no room above white. The map's
histogram is a walk over the map, kept in egui's data for as long as the
picture's map is the same one. The section reads the picture's map
(`Current::picture_face`), so that it stays while the map is shown in the
picture's place, and its button is that switch.

**Display** is `Face::stats`, what the display starts from: lifted through
the gain map at the weight in force, in the working space's primaries — see
[color management](color.md). It is the plot the window, the exposure and
the curve act on, so it is drawn faded to a backdrop, with the response
curve over it always, and the band and the rows under it.

**Output** is the Display plot pushed through the display's response and
encoded on sRGB's curve, over 0 to the surface's room above white
(`FrameInput::room`, which is `App::display_headroom`). It is rebinned, not
scanned: `Stats::scan` counts the same pixels a second time into 4096 bins
on the same axis (`stats::Fine`), and `Fine::rebinned` pushes each fine bin's
value through the response and adds its count to the output bin it lands
in, out-of-range piling into the end it went out of. A handle dragged
changes the response on every frame, and four thousand bins through it is
nothing where two million pixels would be a frame each. Sixteen fine bins
to a coarse one is what keeps an 8-bit file's codes one to an output bin
through a display that does nothing, so the plot does not comb.

The color planes go through the same scalar response as the luminance, a
channel at a time. That is exact where the display treats each channel
alike — a window, an exposure, a clip — and an approximation under the
neutral curve, whose desaturation moves a channel by what the others are:
the planes are then plotted where each channel would go were it gray. They
are read for their shape, and the luminance is still exact.

The rebinned plot is kept in egui's data (`output::binned`), keyed on the
fine bins' address — the `Arc` itself is held beside it, so the address
cannot be another's — the window's offset and gain, the curve, whether a
false color is on, the surface, and its room. Where the room is not known —
an HDR surface whose monitor has not said — it is resolved as the largest
output any value reaches, rounded up to a whole stop above white, and the
key carries what it was resolved to.

## The pixel traced down the column

`histogram::marked` hands every section its `Mark`: the bin its rule stands
on and the words its header writes. A pointer on a pixel of the picture
marks every section at once, each with the pixel as its own stage has it —
the File section reads `Current::sample_as_stored` and writes the stored
value in the file's units, the Gain map section reads `GainMap::stops_at`,
the Display section reads `Current::sample` and writes the value and what
the display makes of it, and the Output section writes that response held
to the surface's room. The four rules down the column are one pixel's path
from the file to the screen.

A pointer on one section's plot is reading that plot's axis and is on no
pixel besides, so only that section marks anything, with the value its bin
stands for. Every hit test is against the rectangles the pass laid out,
which the scroll has already moved, and a pointer over a section scrolled
out of the panel is over nothing (the scroll area's clip rect). A value off
a plot's axis is still written and marks no bar: the rule would otherwise
claim a bar that is not the pixel's.

## The Output section

### What is written in the corners

`Plot::clipped`, asked of the Output plot at black and at white, is the
share of the samples the screen puts out at or below black and at or above
white, per plane, the worst plane reported. Read off the output, it is what
the screen does with the picture, rather than a share worked out again from
the window's two ends on the input. A red flower blows its red channel long
before its luminance goes, and the plane that has piled into the end bin is the one the eye finds
on the plot. The panel writes the two shares in the plot's top corners, in
the accent, over a backing of the plot's own ground so that the number stays a
number on a bar that has climbed into the corner. Only when there is a share
to write: an empty corner most of the time is what makes a number in it news.

The right corner is only written where the surface is actually clipping —
no curve, and an SDR surface, the same test the bottom bar's **clipped**
makes, or a false color, which clips whatever the curve. A curve rolls the
highlights off rather than throwing them away, and an HDR surface shows them;
neither is a share of the picture lost.

The share is exact on a quantized file under a window whose ends sit on
codes: each code lands on a fine bin of its own, and a code pushed to black
or past white lands in an end bin of the output, so the output's share at
either end is the input's share at the window's — a test in `stats.rs`
holds the two to each other. On a float axis the bins are wider than a code and the
share is to the nearest fine bin.

Where nothing is clipped at white because the surface goes past it — an
HDR surface — the right corner says instead how far the axis runs: the
room, which is then the one end of the output's axis that differs from one
screen to the next.

### The marks on the picture

`w`, or the button beside the Output section's band, paints the clipped
pixels on the picture: `MARK_WHITE` and `MARK_BLACK` in
`shaders/image.wgsl`, where any channel of the windowed value is at or
past the bound, painted in place of the pixel and before the false color,
whose ramp does not end in white and black. The whole test lives in the
image shader because that is the one place the windowed value exists per
pixel; the composite pass sees the image target, which for a false-colored
file already holds the ramp's color.

A toggle, like the grid: a way of looking at the picture rather than a
setting of it. `Panels::mark_clipped` is the state, beside the plane and
count-axis toggles, `shader_codes::marks` the two bits, and
`Draw::mark_clipped` the way in — not a field of `Display`, which is kept
per file and restored when a file comes back, where the marks stay on across
files and are left alone by every reset. The key and the button both go
through `App::press(Control::Marks)`, so the two cannot drift.

The button is in the row at the panel's head, set apart from the three
before it by `TOOLBAR_GROUP_GAP`: those choose what the plots draw, and
this one paints the Output band's two ends — the pixels the screen has
taken to black and to white — on the picture. The button wears
Lucide's `triangle-alert` (`icon::TRIANGLE_ALERT`), the sign every editor's
clipping warning wears.

Which ends are marked follows `Display::clips_white`, the same rule the
corner and the bottom bar's **clipped** use: white is only marked where the
surface is actually clipping it.

One channel at an end is enough to mark a pixel, as it is enough for the
corner to count it. A channel that has reached the bound has lost what it
held, and the color left over is not the file's: a raw's white balance
lifts its red well above its green, so a sun's red burns out while its green
is still climbing and the disc comes out pink, and on a warm sunrise the
green may never reach white at all — a test that waited for every channel
painted nothing on a picture whose sun was plainly gone. The paint and the
corner then answer the same question, the corner as a share and the paint
as a place, and a number in the corner is the share of the picture wearing
the paint. The comparisons are exact, as the corner's are.

The test is made in the working space's channels — the color after
`params.primaries` has carried it into BT.709, which is the color `shade`
windows — because that is where the band's two ends are, and where the
corner counts: `Stats::scan` measures the picture there too. A vivid Rec.
2020 or P3 color has a BT.709 channel above white, and one below zero, that
the file never clipped but a surface that stops at either end will, and the
mark says so; [color management](color.md) says why that is a highlight's
clip and not a gamut matter of its own, and what becomes of the color on a
surface with the room. Such a texel is past both ends at once, and the two
marks are painted as a partition of the pixel, so `judge` gives it one:
white where white is being painted, and black otherwise — which is why the
paint's gate, `shader_codes::marks`, is part of `MarksKey`, and a curve
switched on rewrites the chain. The matrix keeps white, so a BT.709 file is
judged exactly as its codes are stored.

The mark is a texel's, not a blend's, and it is kept as one. `judge` in the
image shader is the verdict on one texel of the picture as uploaded, under
the window in force — through the same `load` and `expanded` the draw reads
by, so on lifted, straight color — and everything the marks show is that
verdict, averaged over exactly the texels the picture is averaged over.
Magnifying, a pixel lies inside one texel and takes its verdict outright.
Minifying at the picture's own level, up to the chain's step, `marks_over`
judges the texels under the pixel live, over `area`'s footprint. Farther
out the draw reads a coarse level of the picture, whose texels are averages
that no longer say which of their texels were at an end, so the marks have
a coarse chain of their own (`image_layer::Marks`): `fs_marks` writes its
first level from the picture, each texel the shares of the `STEP` by `STEP`
block under it, and the reducer makes the rest from that as it makes the
picture's chain from the picture, in `Rg8Unorm`, white in one channel and
black in the other. Every coarse level of the picture is bound beside the
marks at the same level, and `shade` wears the paint by the share it reads.
A run of crushed texels stays whole blue at every zoom, a scattered shadow
thins into its neighbors as the crushed pixels thin among them, and nothing
changes at 1:1.

Nothing is stored per texel of the picture — the chain begins at a
sixteenth of it, about a bit a texel — and nothing is stored at all while
the marks are off or nothing coarse is drawn, so a black point dragged at
1:1 costs the marks nothing, and dragged on a fitted view costs one pass
over the picture and the chain from it, the same as the picture's own chain
costs on every frame of an animation. `MarksKey` is what the chain was
written under, the window transform and the lift's weight, and
`ImageLayer::prepare` writes it again when that changes — into the textures
it has, through `Reducer::build_into`, since a window stepped is the same
picture judged again and allocates nothing — and drops it when the texels
change or it is not wanted; every level is rebound only when the chain's
textures change, which a rewrite does not. The
pass reads the picture through a bind group of the chain's own
(`Marks::reading`): the picture's level-1 group names the marks' first level
beside it, and a texture cannot be a pass's target and its input at once.

Two things this is instead of. Judging the resampled color would have found
an end only under a pixel every texel of which is at it: a scattered shadow
vanished the moment the view dropped below 1:1, and at exactly 1:1 came and
went with the rounding, since where a pixel's center falls in the texture is
arithmetic on its coordinate that is not exact at the far end of a large
picture, and `antialiased_nearest` there blends a thousandth of the neighbor
in — shadows that sparkled as the picture was zoomed and went missing at
100%. And reading the marks off the picture's own texels under a far-out
pixel would have cost every texel under it, which is what the chain exists
to avoid. The paint is laid over the pixel's own color, the average of every
texel under it, marked ones included, so a pixel partly marked is a hair off
what painting every texel first and shrinking would give; one wholly marked,
or not at all, is exactly that.

## The Display section

The response curve is drawn always. On a display with nothing asked of it
the curve is the diagonal, which in a panel that plotted one stage would
say nothing the axis under it did not; here the section is the display, and
its curve is what it is read for, a diagonal saying the display does
nothing as plainly as a bend says what it does.

### The rows are every file's

Three rows of settings under the Display section's band — *Exposure*,
*Window*, *Curve* — and every file gets all three, so the section is one
height for every file and for the whole of a file's stay. A section whose
controls jumped from one file to the next, or every time a handle was
dragged past white, would be one no one could keep a hand on.

Three kinds of file use this program — `AutoWindow::default_for` splits on
`Referred`, and the colormaps, the float TIFF and the GeoTIFF keys say who
the measuring audience is — and a graded file's window is 0..1 by rights
where a measurement is windowed to what it holds. Scene light — a Radiance
picture, an EXR — keeps 0..1 too, in the file's own units, and opens with
the exposure already turned: `Display::for_image_with` meters it, putting
the key of the scene at middle gray, and the slider shows the reading in
stops. The rows do not follow that split, because the black handle moves
the window on every file: a graded file's window leaves 0..1 at the bottom
the moment the black handle is touched, so a white handle that meant the
exposure on a graded file and the window's top on a measured one would be a
handle that meant a different thing on the next file along. The two dials
are honestly two. The window is in the file's own units — both handles,
both pairs of keys, the *Window* row — and the exposure is in stops — the
slider, `d`/`f`, `--exposure` — a push on top of whatever the window is,
which is exactly what a meter's reading is, and why a metered file arrives
with the exposure dial turned rather than a fourth kind of window. On a
graded file the *Window* row is where a hand-moved window is put back, *As
stored* being 0..1, as much as where a rule is chosen.

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

The three windows are named for what they do — *As stored*, *Full range*,
*Trimmed* — rather than for the rule that does it, which the tooltip says,
and the bottom bar says the same in one word each: *stored*, *full*,
*trimmed*, and *manual* once a handle has moved. There is no fourth button
for "the image's own", because the file's own is always one of the three —
*As stored* on a graded file and on scene light, *Trimmed* on a measurement
— and the reset button puts it back, the meter's exposure with it.

### The band is the levels track

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

The value under the hand is written in the section's header rather than in a
tooltip. egui takes a tooltip down for the length of a drag, and a drag is
exactly when the number is wanted.

### What goes dead

The row of curves is dead under a false color, and says why when rested on.
The compositor holds the curve at a clip there — `Display::curve_on` is
where the choice is made, and `composite.rs` and the pointer's readout both
ask it, the bar's words `Display::false_colored` — because a ramp has no
color past its end for a
highlight to roll off into, and a curve over the ramp would bend the very
mapping the reading is being taken off. `Display::response` runs the curve
the compositor runs, the buttons refuse the press and `t` does nothing, and
the row stays where it is — the section's height is the file's — rather
than leaving. A panel that lit whichever curve was chosen and let the buttons
change it would have a press change nothing on screen and then change the
picture some time later, when the ramp came off.

### The exposure is a slider

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
## How a plot is drawn

The ends of the input's and the file's axes are written in their plots' top
corners, in the dim ink on the plot's own ground, only where they are not 0
and 1 decoded — a
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
being themed. A bin is a point, which fixes the panel's width and is what
the pointer reads the plot by. What is drawn is one column to the device pixel (`icon::Grid::columns`),
each as tall as the fullest bin under it (`plot::bins_under`,
`plot::tallest`): a column to the bin lands half its edges mid-pixel at a
fractional scale, and the feathering shows as a comb, where a column to the
pixel is even at any scale; and taking the fullest bin rather than the mean
keeps a spike from being averaged away where a column is wider than a bin.
At one device pixel to the point the two are the same picture. The bands
under the plots and a ramp's swatch are cut the same way, which is why
nothing here smooths the plot.

