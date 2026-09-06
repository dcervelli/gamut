# The interface

## Opening

The first file is shown stretched to fit the space the interface panels leave
in the middle, and the window opens at the image's own size plus that chrome,
shrunk to fit the monitors (`src/app/window.rs`). `` ` `` hides the panels and
gives the image the whole window, re-fitting it as it goes.

Every monitor is asked, not one. `primary_monitor` is `None` on Wayland by
definition — there is no such thing there — and nothing before the surface is
mapped says which monitor the compositor will choose. So `window_size` asks
each monitor what window it would want, being the image held inside
`MAX_WINDOW_FRACTION` of that monitor's room, and takes the largest of those
answers that fits on *every* monitor. A window sized that way cannot overrun
whichever screen it lands on. Where none of them fits everywhere — a monitor
smaller than `MIN_WINDOW`, say — the smallest answer is taken as the least bad
of them.

The whole calculation is in logical pixels, which is the correction that
matters. The image's own pixels are physical, so a monitor's scale converts
them; the panel constants are logical already. Asking in physical pixels does
not work, because winit's Wayland backend converts the size a window is
created with at a scale of `1.0` — the surface has none until the compositor
configures it — so physical pixels are taken as logical ones and the window
opens `scale` times too large, well past the screen on a 4K monitor at 2×.
The scale used is the output's integer one, 2 where the compositor is really
running 1.6; the true fractional scale only arrives with
`wp_fractional_scale_v1` after the surface is mapped. That error goes the safe
way, opening a little under 100% rather than overrunning.

A window also opens no smaller than one the interface itself fits in.
`ui::PANELS_ROOM` is the content area the histogram and the information column
need together — the strip's width, the plot's fixed height, the gap, and the
least column the panel will show — and `PANELS_WINDOW` is that plus the chrome
and a logical pixel of slack. A window opening below it would have both those
toggles dead in it from the first frame, which is not something the viewer
asked for; where the picture is smaller than the interface, the window is
better a little larger than the picture. The floor is measured against the
monitor's whole room rather than against `MAX_WINDOW_FRACTION` of it — the
fraction is about leaving the desktop its share of a large window, and this is
about a small one being usable at all — and a monitor that cannot take it is
given `MIN_WINDOW` instead, since a floor that did not fit the screen would be
the very thing the rest of this prevents.

The slack is a rounding allowance, not a margin. A window is laid out in
logical pixels and sized in device ones, so the size that comes back is the
size asked for rounded to the device grid: 392 logical pixels on a monitor at
1.6 is 627 device pixels and 391.875 logical ones, a hair under the 392 the
panels needed. Asking for exactly the room leaves them out about as often as
not; asking for a pixel more never does, half a device pixel being the worst
the rounding can do.

`--size <W> <H>` replaces the calculation with the two numbers it is given.
They are the whole window, chrome included, in the same logical pixels.
Neither the image nor the monitors gets a say afterwards: a window larger than
the screen is something a compositor is asked for on purpose, and only a floor
of `MIN_WINDOW` applies, below which the chrome would have all of the window.
Whether the request is honored is the compositor's business — a tiling one
uses it as the floating size, if it uses it at all.

## Keys and buttons

The key table itself is in [KEYS.md](../user-docs/KEYS.md); what follows is why
the controls are shaped the way they are.

The copies have a button too, at the top of the left strip: it opens a menu of
what can be taken — the file's name, its path, its URI, everything the
information panel says about it, and the picture itself — each cell doing
exactly what its key does, and named in the tooltip by the key table's own
words for it. The two copies of the pixel under the pointer are not on it:
while the menu is open the pointer is over the menu, and there would never be
a pixel under it to take.

`Ctrl+V` has a button as well, under that one, and it is on screen only while
the clipboard is holding a picture that can be shown — the clipboard is looked
at on the same quarter-second cadence as the file and the palette, and not at
all while the interface is hidden. A button that did nothing when pressed would
be worse than no button, and one that comes and goes says what the clipboard
holds without being asked.

The panels are opaque and the image is fitted inside them rather than passing
behind them, so `` ` `` changes how much room a fitted image has and it re-fits
on the spot.

`` ` `` has a button as well, in the corner of the top bar, and Shift held on
it is `~`. It is the one control that hides itself, so the first time the bars
go a message says which keys bring them back — once and not again, since a
message every time would be in the way of the picture that was just asked for.
`Esc` is one of the two because it is the key that puts things away everywhere
else in the window; without it, someone who pressed the button and never read
`--help` would have nothing to press.

A copy takes the selection and leaves the picture exactly as it was, which
makes it the one action in the program with no sign that it happened — a copy
that worked and a key that was never read look identical. So each one raises a
message at the foot of the content area saying what was taken: the name, the
path, the URI, the picture, a field of the information panel, the pixel under
the pointer. It goes on its own after 2.6 seconds, and can be taken off sooner
by the cross it carries or by `Esc`, which puts away whatever is up — a menu
first, then the interface if it is hidden, then a message — and only quits when
there is nothing left to put away. The interface comes before the message
because the message raised when it went is the one that says `Esc` brings it
back, and a key that dismissed its own instructions would leave the reader with
a window they could not get out of. `q` quits regardless, since a copy is often
followed straight away by it.
The message has three levels, and the theme's own inks carry them: the
ordinary text for something done, the theme's yellow for something that could
not be done and broke nothing (no pixel under the pointer), its red for a
failure. Copying the picture is the one copy that cannot say anything at once
— it walks every pixel and encodes a PNG on a thread of its own — so that
thread sends its outcome back and the message appears when the clipboard
actually holds it.

Zooming leaves fit mode; panning does not, so Space then Down scrolls through a
tall image at fit-width.

A pan or zoom asked for by name — a key, a notch of the wheel, a choice from
the zoom menu — is a move of 200 ms rather than a cut, and one asked for
before the last has landed starts from wherever the view has got to, with the
whole 200 ms again to finish. A drag, `Shift` with an arrow, and a trackpad's
scroll are not moves: the hand is on the view, and it goes exactly where it is
put. The path a move takes is a straight line in the space-scale diagram of
Furnas and Bederson (*Space-Scale Diagrams: Understanding Multiscale
Interfaces*, CHI '95): the view's center is interpolated in (pan × zoom, zoom)
rather than in pan and zoom separately. Interpolate those separately and a
zoom that also pans swings the image out to one side and back, the zoom
carrying the target away faster than the pan brings it in. Along the
space-scale line every point of the image crosses the screen in a straight
line at a steady rate, and whatever a wheel zoom is anchored on stays put from
the first frame to the last. Apart from the copying chords and `Ctrl` with an
arrow, keys held with Ctrl, Alt or Super are ignored, so window-manager chords
such as `Super+0` do not disturb the view.

Every file that has been on screen is remembered as it was left — its pan and
zoom, and everything the display was doing to it — and stepping back to it puts
all of that back. Flipping between two pictures is how they are compared, and a
comparison that reset one of them each time it came round would compare
nothing. The memory is kept by path, so it survives the list being rebuilt
under a directory that is being written to.

`]` and `[` keep the pan and zoom of the picture on screen when the file they
land on is one they have not opened before and is the same size — a directory
of frames or of exposures is a set to be compared, and the comparison only
works if the same detail stays under the same pixels. A file of another size
that has not been seen is a different picture, and is fitted.

Every display control is also a start-up flag — `--colormap viridis`,
`--tone-map neutral`, `--window minmax`, `--exposure -1.5`, `--output hdr`,
`--histogram`, `--info`, `--no-minimap` — which is handy for scripting and for
comparing two files side by side.


## Minimap

The minimap is on unless `--no-minimap` turns it off. `m`, or the button at
the foot of the left strip, puts a thumbnail of the whole image in the
bottom-left corner — the corner that button is in, and the one the two panels
describing the file leave alone, they being down the right — with the part of
it on screen picked out and the rest washed over. It is the map to read while
zoomed in far enough that the image on screen no longer says where in the
picture you are.

On by default because it costs nothing until it is wanted: it stays off screen
for as long as the whole image is in view, so the first zoom that cuts
something off is the moment it appears, and that is the moment it is useful.

It only appears while some of the image is off screen. A view holding all of
it is already its own map, and the thumbnail would be a smaller copy of the
window laid over the corner of it, so the widget leaves and comes back on the
zoom that first cuts something off. The toggle keeps its state through that:
the button stays lit, and the minimap returns without being asked for again.

The thumbnail is not a separate rendering of the image: it is a second quad in
the image layer's pass, drawn from the same texture through the same shader as
the view itself, reading whichever coarse level suits the size it is drawn at.
Exposure, the display window, false color and tone mapping therefore reach it
without any of that being reimplemented for a widget, and it costs one more
draw call and a second uniform. Only the border and the wash over what is off
screen belong to the interface, which is why both are drawn hollow or
translucent — the thumbnail underneath them is in the layer below.


## The chrome

The chrome is four panels: top and bottom bars spanning the full width, with
skinny left and right strips nested between them, so the corners belong to the
bars and the strips never reason about where one ends. `Chrome` derives all
four from the window size alone, which is what lets the frame builder and the
click handler agree on where a widget is without either of them owning it. The
top bar carries which file it is — its place in the list, in front of its
name, so that the count is always in the same place whatever the name is — and
what the image is: its size, its pixels, its color space, all fixed for as
long as the file is on screen. The name is the only thing in the window set
bold, and the only thing drawn in the ink the theme keeps for it; the count in
front of it is set like the facts at the other end of the bar, since it is one
of them. Picking out two things picks out neither, and what a reader wants
from that bar at a glance is the name. The bottom bar carries what changes: what is
under the pointer, and what the view is doing to the image, the last of which
says nothing at all while nothing is being done. The pointer's end of it is a readout of one pixel —
where it is, the components the file holds there in the file's own units, the
values the display window maps them to, and a swatch of the color they come
out as. The two numbers answer different questions, which is why both are
there: the stored one is the measurement, the mapped one is why it looks the
way it does. Saying what the screen is showing means running the display
transform on the CPU, so `ToneMap::apply` and `Colormap::color` in
`image/display.rs` mirror the shader functions of the same names — a swatch
that disagreed with the image beside it would be worse than none.

The strips are a bar's thickness wide — they hold a column of square toggles
and nothing else, so a frame of even weight is the right one — and the left one
holds the copy button above the paste button, the right one the histogram above
the file information, the order the two panels they open are stacked in over
the picture. The button that comes and goes with the clipboard is the last of
the left-hand column, so that nothing above it moves under the pointer as it
appears. The minimap toggle is in the left strip too, but it comes up from the
foot of it rather than down from the top: it is the one toggle whose panel has
a corner of its own, and it sits in that corner. The room the column above it
would take is kept clear whether or not the paste button is on screen, so the
toggle stays where it is as the clipboard changes, and a window too short for
both ends of the strip drops it rather than standing it on the column coming
down. The end of the top bar holds the button that hides the interface, and
inside it the zoom percentage, a readout that is also a button. The percentage
is a measurement of the picture, which is what the top bar is for, and the
button that is not one sits outside it in the corner of the window, where the
world already looks for a control of that kind. The head of the bottom bar
holds the other readout that is also a button — the grid toggle, which says
whether the grid is drawn and how far apart its lines are — and after it the
button at the head of the pixel readout. A grid laid over the picture is
something being done to it rather than a fact about the file, so it belongs in
the bar that carries what is being done; it is in a bar at all rather than in a
side strip because it has a spacing to read out, and the strips are a button
wide, which is too narrow for words. Its mark leads the bar and its reading
follows the mark, the two being one reading rather than a label and a button
sharing a square. The bars are inset at their ends by the same margin that centers a toggle
across a side panel — derived from it, not merely equal to it — so the last
button in a bar ends on the same line the column of toggles below it ends on,
and the file name starts on the line the left-hand column starts on. Two edges
a few pixels apart
read as a mistake in a way that one shared edge does not, and deriving the
margin is what keeps them from drifting apart when a button size is retuned. A
menu hangs from the button that opens it, off whichever of its edges faces into
the window: down from a button in the top bar, up from one in the bottom bar,
and out to the right from one in the left strip, where a button has its
neighbors above and below it and its room to the side. Pressing the percentage
opens a menu of zooms — the ladder from 10% to 1600% and the three fits as
icons — hung from the button, its right edge in line with the button's, placed
against the window rather than against the frame the picture is in: a menu
pushed around by where the image happens to be would not stay under the thing
that opened it. The readout is a fixed width so that the click handler knows
where it is without measuring what it says, and so that it does not shuffle
along the bar as the zoom changes. The grid toggle is the opposite: it is
fitted to the reading in it, so it widens when the grid comes on and pushes
the pixel button and the readout after it along the bottom bar. What moves
there moves on the press that was just made, and the mark the press was aimed
at is the one thing that stays where it was.


## Panels a window has no room for

The two panels down the right of the window — the histogram and the
information column — are both `PANEL_WIDTH` wide, and the histogram is one
fixed height besides: its plot gives a bin to the logical pixel, and the rows
under it are set to what they say, so there is nothing in either to give. A
content area smaller than one of them gets no panel rather than one drawn over
the picture it is about and off the edge of the window. `ui::room` asks the
question for both at once, because they are stacked: the histogram takes the
top of the strip, and what it takes is height the column below it does not
have, so a window can have room for the column alone and none for it under an
open plot. What it takes is settled inside `info::panel`, which asks whether
the plot is on screen rather than whether its toggle is on — a window too
short for the plot is not one the column has to start below, and putting that
question in one place is what keeps the frame builder and the pointer from
disagreeing about where the column begins.

One answer serves three readers — the frame builder, `layers::hit` and the
application — since a panel the pointer could reach but the frame did not draw
would take presses meant for the picture under it. It also decides the two
toggles in the right-hand strip: where there is no room for what one opens it
is drawn dead, in the ink the surface switch uses when there is no headroom to
switch to, and the press is refused rather than quietly setting something no
one can see. Its tooltip says why instead of naming the panel and the key
beside it — `tooltip::NO_ROOM`, a sentence rather than a label, because what a
dead control owes the reader is the reason and not the binding. The surface
switch is answered by the same `tooltip::disabled`, out of `App::hdr_state`
rather than out of the room: it has two reasons to be dead and they want
different sentences, one of them naming the `--output hdr` that would change
the answer — see [color](color.md). The toggle
stays in the strip either way: a control that is sometimes there is a control
that has to be found again.


## Layers and the pointer

A frame is drawn in two layers, and the menu is the only thing on the second.
Shapes keep the order they were emitted in, but a layer's glyphs go down after
all of its shapes — the text pass is prepared whole, and one per layer is what
it costs — so without a layer above them the words on a panel would show
through anything laid over that panel, which is what an open menu does to the
histogram's axis label. Two layers is as far as this goes on purpose: it is
the smallest thing that gives the interface a front, and each one costs a
glyph pass whether or not it has any words on it.

The pointer reads that stack back. `src/ui/layers.rs` puts the window in
order — the picture at the bottom, the panels that float over it, the chrome
around it, and whatever menu is open on top — and `hit` walks it from the top
down and names the first layer to claim the point. One answer serves the
highlight, the press, the wheel and the bar's pixel readout, so the four
cannot disagree about what is under the pointer; before there was one, they
were four orderings written out separately, and they did disagree — the zoom
menu is drawn over the two panels down the right of the window, and a click on
a cell that happened to be over one of them went to the panel instead. Every
layer is opaque: a press that lands on a panel is spent there whether or not
it hit one of that panel's buttons, so nothing reaches what is drawn behind,
one gesture never acts on two things, and a widget that lights up under the
pointer is a widget the next click will actually press. The stack is derived
from the window size and what is on screen, the same few numbers the frame
builder lays out from, so what the pointer reaches is what was drawn under it
without either side owning a cached layout.

Being over a layer and being taken by one are still two things. A menu takes
the pointer for as long as it is open, as menus do everywhere: a press
anywhere off it dismisses it rather than reaching what it landed on, the wheel
is spent on it, and nothing behind it lights up. That grab is applied by the
handlers over the top of the stack's answer rather than folded into it, which
is why the bar goes on reading out the pixel under the pointer while a menu is
open — what the pointer is over has not changed, only what may be pressed.


## The information panel

The information panel (`src/ui/info.rs`) is as wide as the histogram — one
constant, fixed by the histogram's need for a bin to the logical pixel — so
the two line up down the right of the window, and its column is measured
inside a gutter kept clear for the scrollbar whether or not there is anything
to scroll: text that reflowed the moment the bar appeared would be text that
reflowed as it was being read. It starts under the histogram when that is
showing and at the top of the content when it is not, and the two share one
ground — the bars' own surface, mildly transparent, carrying the bars' own
ink. The
pointer belongs to it while it is over it: the wheel scrolls the column
instead of zooming, and a press starts a drag of the scrollbar's thumb
rather than of the picture, moving the column by what putting the thumb there
would rather than by what the pointer traveled — one thing or the other for
as long as the button is held, so a drag that runs off the panel goes on
scrolling rather than beginning to pan half-way through. A column with nothing
left to scroll to still takes the gesture rather than handing it back, and
makes no closed hand for a drag that would move nothing.

What it says comes from three places, and is written under headings that keep
them apart — a column this long is read by looking for a thing rather than by
starting at the top. The file's own facts — its name, its path, which decoder
turned out to own it, when it was written and how large it is — are one `stat`
and one look at the header, taken as the image goes on screen. The picture's
own are what the decoder already said: its size, what each pixel holds, the
color space those numbers are meant in, and what the GPU stored them as. The
bars say some of that as well, but they say it in passing and drop it when the
window narrows, and a fact worth reading is a fact worth being able to go back
to.

The rest is its EXIF, read by `src/image/exif.rs` on the loader thread beside
the decode, because it is one more parse of a file somebody else chose the
bytes of and that is the thread with the panic guard around it. What comes
back is already words, and already grouped: the fields a photograph is read by
— camera, lens, when, the exposure as one line, the focal length with its
equivalent — then where it was taken, the coordinates in degrees a map will
take with the rest of the GPS directory under them, then whatever somebody
wrote in words, and last everything left over. That last split is had for
nothing: TIFF's own tags describe the file and the Exif directory describes
the shot, and every tag says which directory it came from — so the long tail
is grouped by asking each one rather than by a table of where each belongs. A
group that came to nothing is not carried at all, an empty heading being a
question about where the rest of it went. Nothing there is a tag number or an
offset by the time the interface sees it.

The panel is also the one part of the interface that is read out rather than
merely read. A click on a field copies it, a click on a heading copies the
section under it, and a button in a header above the column copies the lot.
Each takes as much of a table as what was clicked actually is: the whole
panel is three columns, a row of it having to name its section to be worth
anything beside a row from another; a section is two, every row of it having
come from the one that was clicked, so that naming it down the column would
be saying once per row what the click already said; and one field is not a
table at all but the value, as it is written. What is offered is plain text:
it is CSV in what it says rather than in how it is offered, because a copy is
bound for somewhere else and every place words can be pasted takes those,
where CSV alone would paste into a spreadsheet and nowhere else. Where there
are columns to keep apart, a value holding a comma — a size with its digits
grouped, a coordinate, half of what a raster says about its ground — is
quoted, or the row would not survive being read back; a field copied on its
own is not, there being nothing for it to run into.

Which means three lists rather than one, each derived from the last: the
words, then where they go, then what can be pointed at. One index runs
through all three, so what is under the pointer, what is drawn lit and what
lands on the clipboard cannot come to disagree. The button that says what a
click would take appears over the words rather than beside them, on the layer
above so that it covers them — the column is as wide as the panel lets it be,
and there is no margin to stand a button in — and nudged back inside the panel
where being centered on what it copies would hang it over an edge. A press on
the panel starts a scroll of the column as well, the two gestures being one
and the same at the moment the button goes down, so the copy is made only if
the button comes back up without the pointer having gone anywhere. And the
button goes away while the column is scrolling: the pointer is not moving,
the words under it are, and a button that followed whichever of them happened
to be passing would blink from field to field all the way down.

A raster is read through a different handful of fields, and they are not EXIF
at all. GeoTIFF shares the TIFF directory rather than taking a container of
its own, and packs a directory of *keys* into one tag with two more holding
the values that will not fit in a short, so `src/image/geo.rs` takes that
apart: the coordinate system as the file names it and files it, the size of a
pixel on the ground, the corner the raster hangs from, and the ground it
covers — which is the corner and the pixel count multiplied out, and comes to
the same four numbers `gdalinfo` prints. The extent goes down the panel as one
row per axis, named for what the axes are: a pair of seven-figure spans will
not fit on a line this wide, and a coordinate broken across two lines is a
coordinate misread. The tiepoint is walked back to the
corner where it is not already there, the matrix form is read where a file has
that instead, and a raster whose axes are turned off the model's is given its
corner and told plainly that there is no rectangle to quote. Nothing consults
a coordinate-system register: EPSG:2056 is quoted as EPSG:2056, beside
whatever the file calls it, because turning that into a datum and a projection
means shipping the register that defines them.

Two kinds of raster need one more step to be read at all, and both take the
same one. A BigTIFF — the same tags and types with eight-byte offsets, which
is how anything that might pass four gigabytes is written, and how a plain
elevation model is written whether or not it needs to be — is a form the
metadata reader does not know, EXIF being defined on the original; and an
ordinary TIFF written straight through, with its directory after its pixels,
keeps that directory past the end of the prefix. `src/image/directory.rs`
answers both the same way: the TIFF decoder is already in the tree, reads both
forms, and seeks to the directory wherever it is, so it reads the directory
and this writes what it found back out as an ordinary block in memory — the
values, none of the pixels. Everything downstream is then one path for every
file rather than a second kind of directory rendered a second way. What
cannot survive the trip is left out rather than written wrongly: a pointer to
another directory in a file the block is not, a number too wide for the type
it would have to be written as, a list that cannot agree what it holds.

The other half of reading a raster is naming what it holds. The metadata
standard describes what a photograph carries and no more, so the rest of TIFF
6, the tags GeoTIFF and GDAL park in the same directory, and the compression a
raster is actually stored in all arrive as numbers — a column of `TIFF tag
33550` says what is in the file without saying what any of it is. A table of
names covers them, and the compression codes are named here rather than left
as "reserved compression 5", which is what the EXIF renderer calls the LZW the
format has meant since 1992.

Three things had to be decided rather than read. Numbers are rewritten to the
digits they are worth: a file storing an aperture as 89/50 means exactly 1.78,
and quoting it back as f/1.7799999713880652 says only that a rational went
through binary floating point. Bulk values are left out — a maker note or a
table of strip offsets is a fact about the file's layout, not about the
photograph, and rendering one costs the memory of the string as well as the
room. And a TIFF is read as a prefix rather than as a file: a TIFF *is* its
own metadata block, with no chunk to seek to, so the parser reads the whole of
whatever it is handed and an elevation model would be pulled into memory for a
date. What it is handed is the first 8 MB, which is enough because of where a
directory goes — a 443 MB scanned map keeps its first directory at byte 8 with
every value inside the first 10 kB, which is what any writer that means the
file to be read out of order does, and the fields come back in four
milliseconds. Offsets that run past the prefix are expected rather than
exceptional, so the parse is asked to continue through them and hand back what
it did read. Every other container carries the block in a chunk that is found
by scanning headers, and costs a few hundred microseconds.

Taking the block from the decode instead was the obvious other answer, and it
is worse. The decoders that could give one cheaply — WebP already reads it for
the orientation, JPEG holds the file whole — are the ones that cost nothing to
re-read. The one that would benefit cannot: the TIFF decoder has the directory
parsed, but the crate behind it will not follow the sub-directory pointers the
exposure, the lens and the coordinates live behind, so a camera TIFF would
come back with less than a second read gets, and its values would arrive as
numbers needing a second renderer to say what they mean.

It is the one part of the interface with more to say than fits, and so the
only part that scrolls. Its column is laid out in full every frame and the
scroll is subtracted from each row's place down it, which leaves rows lying
above and below the panel. Nothing here clips them: the text
layer takes a rectangle to cut the glyphs to, which glyphon trims the quad and
its texture coordinates against together, so a line sliding under the panel's
edge is drawn as much of a line as is still inside. Measuring a paragraph
before drawing it is the same call that draws it, one width and one wrap, so
what the scroll is clamped against is the height the text actually comes out
at rather than an estimate of it.


## Popups and menus

Popups are `render::ui_layer::Popup`: a panel of cells under named headings,
anchored to a corner of an area, which answers where the panel goes, where
each section's name is set, where each cell landed, and which cell a point is
over. What a cell has in it and what pressing one does stay with the caller
(`src/ui/menu.rs`), so a second menu is a `Menu` variant, its sections, and
the code that draws its cells. Only one can be open, which is what makes
dismissing one unambiguous: an open menu takes every press before the chrome
and the image do, a press on a cell chooses and closes, and a press anywhere
off the panel is spent closing it. `Esc` closes it too, in front of the quit
it would otherwise be.

A cell's width belongs to its section rather than to the panel: the panel is
cut for the widest row any section asks for, and every other section lays its
own cells from the same left edge and stops where they stop. A short row is a
short row, not three cells stretched to the width of four. The height is the
one thing held uniform, since cells of a height read as one panel.

That is what lets one menu hold things that are not the same kind of thing.
The zoom menu holds three: **Zoom**, eight percentages four to a row; **Fit**,
the three fits as arrows; and **Up-scaling**, the magnification filter as the
two words `Nearest` and `Bicubic`. Undivided, those last two read as a fourth
fit — and there is no picture of "bicubic" a reader arrives at unaided, so
theirs are the one pair of cells cut wider than the rest, by exactly what the
words need. `ZOOM_SECTIONS` is `ZOOM_CHOICES` cut into three and a test holds
the two in step, since a choice in no section could never be pressed.

They are opaque, and the image is drawn in the `Viewport` they leave rather
than behind them: zoom, fit, pan limits and the wheel's anchor are all measured
against that rectangle. It is derived per frame from the window and whether the
panels are showing, never stored, so `` ` `` re-fits a fitted image without
anything having to notice that it should.

