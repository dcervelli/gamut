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
information panel says about it, and the picture itself — each item doing
exactly what its key does, printing that key beside it, and named in the
tooltip by the key table's own words for it. The two copies of the pixel
under the pointer are not on it: while the menu is open the pointer is over
the menu, and there would never be a pixel under it to take.

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
four from the window size alone, before egui lays anything out, which is what
lets the picture be fitted into what they leave without waiting a frame on
the toolkit. The
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
holds the copy button, the open button under it, the region button under
that and the paste button under that, the right one the histogram above the
file information, the order the two panels they open are stacked in over the
picture. The first two are together because they are one gesture — this file,
handed to something else — and the button that comes and goes with the
clipboard is the last of the left-hand column, so that nothing above it moves
under the pointer as it appears. The minimap toggle is in the left strip too, but it comes up from the
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
opens a menu of zooms — the ladder from 10% to 1600% and the two fits as
icons — hung from the button, its right edge in line with the button's, placed
against the window rather than against the frame the picture is in: a menu
pushed around by where the image happens to be would not stay under the thing
that opened it. The readout is a fixed width so that it does not shuffle
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
question in one place is what keeps the two panels from disagreeing about
where the column begins.

One answer serves both readers — the interface and the application — since a
toggle that quietly set something no one could see would be worse than one
that does nothing. It also decides the two
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


## The region

A region is a `Region` in `src/image/region.rs`: a rectangle of whole image
pixels, half-open, never smaller than one. It lives in image pixels because
it is about the file — the pixels copied out of it are the same pixels at
any zoom — and in the data model because that is what it is a rectangle of;
`image::encode::displayed` takes one and walks only what it holds, so a copy
of a selection is a crop of the copy of the whole and cannot come out
rendered differently. Every change to one is a pure function there — drawn
from two corners, moved, pulled by a handle, grown, nudged — each clamped to
the image, and each tested on its own with no interface in the way.

What the application holds is a `Selection` — off, asked for, or drawn —
and, during a drag, a `Grabbing`: what was taken hold of, the region as it
stood at the press, and where the press was in image pixels. Every frame of
the drag remakes the region from those three and the hand's place, rather
than from the frame before, so a drag that goes off the picture and comes
back has nothing accumulated in it. The interface reads all of this from
`FrameInput` and never writes it: what it hands back is `Command::Grab`,
`Pull` and `Release`, and `App::act` does the rest.

The drag is classified where the button went down, not where the pointer is
when the toolkit calls it a drag. egui defers the decision until the pointer
has moved six points or the button has been held most of a second, and by
then the pointer is off the press — so `Pass::region_gestures` reads
`press_origin` and tests that against the handles. With a region asked for,
any drag draws one; with a region on screen, a drag from a handle pulls it
and a drag from inside moves it; a drag from anywhere else is the view's, as
it always was, which is what keeps a picture navigable under a region
larger than the window. The hand's place goes back to the application in
image pixels on every frame of the drag, through the same placement the
bar's readout uses, because the application's own pointer stands still for
the duration: egui consumes the pointer events of a drag it holds, and
`App::window_event` does not update `Pointer::cursor` for a consumed event.
The drag ends when egui says the response is no longer dragged, which covers
the button coming up and Escape aborting the drag alike; a release the
application never heard about would leave it holding a drag that was over.

The region is painted in `src/ui/region.rs` on the picture's own painter,
under the floating panels, rather than in an area of its own: an area takes
the pointer from what is under it, and the picture's response is what the
drag on a handle is read off. The eight handles are placed on the device's
grid through `icon::Grid`, like every other thin thing over the picture, and
hit-tested with a little reach past their edges; a corner is asked before
the edges it overlaps on a region drawn small, since it moves two edges
where they move one. Which handle the pointer rests on goes back each pass
as `Command::OverGrip`, a pass late like `OverImage`, and that is what the
arrows consult: with the pointer on a handle they move the handle a pixel,
and otherwise the region. An arrow along an edge — Up on the right edge's
handle — moves the region rather than doing nothing, so no key is dead while
a region is up. `Ctrl` with an arrow grows that side. None of it is animated:
a region moves a pixel at a time, and a pixel has nothing to animate.

The region wears its measurements while the pointer is on it: its size at
its middle, and each edge's coordinate inside the mark in the middle of that
edge. They come and go with the hand rather than with a clock, since the
hand is what says which region is being worked on, and a region left on the
picture keeps only its outline, which is the thing it is for. `App::over_region`
is the reading, and it is `Pointer::grip` — the hit test the interface
already reports, so a panel covering the region does not count — or a drag
in flight, since a corner dragged to the edge of the image leaves the
pointer off the region it is still resizing and the size is exactly what is
being watched then.

`ui::region::labels` lays the five of them out and is where the fitting is
decided. Each is a pill, and one that will not fit inside the visible part
of the region, or that would land on a pill already placed, is left out
rather than written over the outline or over its neighbor. They are given
room in one order — the size, then left, right, top, bottom — so the size is
the last to go and a region drawn small says the one thing worth saying. The
size is centered on the part of the region that is on screen rather than on
the region, since a region larger than the window has its middle wherever it
has it; the coordinates are placed against the true edges, so an edge off
screen simply has no room and loses its own. The coordinates are the
boundaries rather than the pixels beside them — the right minus the left is
the width written at the middle — because two readings that did not add up
would be worse than either alone.

`Space` fits the region through `View::fit_region`, which sets the zoom the
region's own size asks for and centers it, and leaves the view out of fit
mode: a fit is a zoom the viewport decides for the whole picture, and this is
one chosen for part of it, so it is held as `1`..`5` are held and does not
follow the window. Which of the two fits comes next is `App::region_fit`,
kept apart from the view for the same reason.

`Esc` takes the region off after a message and before quitting: a message is
about what was just done, the region is what was being done to, and the one
that stops being news first goes first. Stepping to another file takes it
off, and so does the file coming back a different size — the pixels it
marked out are no longer the pixels — while a reload at the same size keeps
it, being the same picture read again. The key is `x` rather than `k`, which
the histogram's planes toggle already holds in the j/k/l run.

## Layers and the pointer

The window is a stack, and egui keeps it: the picture at the bottom, laid out
as the one response the central panel holds; the areas floating over it — the
minimap, the histogram, the information column — at `Order::Middle`; the
message about what was just done in the foreground; and whatever menu or
tooltip is open above the lot. egui routes the pointer by that stack, so the
highlight, the press, the wheel and the picture's own drag cannot disagree
about what is under it. Every floating area is `interactable` and takes the
press whether or not it landed on one of that panel's buttons, so nothing
reaches what is drawn behind, one gesture never acts on two things, and a
widget that lights up under the pointer is a widget the next click will
actually press.

The one reading of the stack that is ours is the bar's pixel readout: it
asks whether the pointer was over the picture with nothing between, which
the picture's response answers each pass and hands back as
`Command::OverImage`. It is a pass late only when a panel has appeared or gone
under a still pointer, and that pass is being painted anyway.

A menu takes the pointer for as long as it is open, as menus do everywhere: a
press anywhere off it dismisses it rather than reaching what it landed on,
and the wheel is spent on it. That is egui's popup doing what it does; what
the application adds is that `Esc` and `q` ask it first — `App::close_menus`
— so that the key that puts things away takes a menu off before it means
anything else, and `q` never quits out from under one.

The chrome around the picture is egui's panels, given exactly the sizes
`Chrome` works out from the window: the viewport the picture is fitted into
is derived before the interface is laid out, so a fit never waits on a pass
of the toolkit. Every event goes to egui first, and one it takes for itself —
a press on a button, a wheel over a panel — goes no further. Keys do not: the
key table binds some of them by where they sit on the keyboard, which egui's
keys cannot say, so they stay on winit and go to egui only while one of its
text fields has the focus. For that to hold, no control may take the focus a
click would give it: a focused button swallows every key after it, so every
control here senses a click and nothing more.


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

Which means one list, `Contents`, and one index run through it for the
drawing and the copying alike, so what is drawn lit and what lands on the
clipboard cannot come to disagree. Each heading and each field is a block
that senses a click, and the button that says what a click would take appears
over the words rather than beside them — the column is as wide as the panel
lets it be, and there is no margin to stand a button in — nudged back inside
the panel where being centered on what it copies would hang it over an edge.
A press on the panel is the start of a drag of the column as well, the two
gestures being one and the same at the moment the button goes down; egui's
scroll area is what parts them, reporting a click only for a press that came
back up without having traveled.

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
only part that scrolls, which is egui's scroll area's to do: the bar down the
panel's inner edge in a gutter kept clear for it whether or not there is
anything to scroll, since text that reflowed the moment the bar appeared
would be text that reflowed as it was being read. The scroll area is the
file's own — its identity is salted with the path — so stepping to another
file starts at the top of its column rather than however far down the last
one had been read.


## Popups and menus

Popups are egui's, hung off the button that opens them in `src/ui/chrome.rs`
and aligned to it — below the zoom readout, above the pixel dot, beside the
copy button and the open button under it — with what each holds laid out in
`src/ui/menu.rs`. Only one is
open at a time, a press on a cell chooses and closes, and a press anywhere off
the panel is spent closing it. `Esc` closes it too, in front of the quit it
would otherwise be. A menu that does not fit where it was hung is moved into
the window by egui rather than withheld, which is the one thing here the
display list did differently: it refused to open a menu the window had no
room for.

Each cell is a typed `Control` — a `ZoomChoice`, a `PixelFormat`, one of the
`Copies` — so that a press comes back as exactly what was chosen, and the
tooltip on it is named from the key table by the same `action_of` that names
a button: a numbered cell by the key that goes to that zoom, a fit by the key
that toggles the fit, a copy by the very line that describes the copy.

The zoom menu holds three kinds of thing, and says so: **Zoom**, eight
percentages four to a row; **Fit**, the two fits as arrows; and
**Up-scaling**, the magnification filter as the two words `Nearest` and
`Bicubic`. Undivided, those last two read as a third fit — and there is no
picture of "bicubic" a reader arrives at unaided, so theirs are the one pair
of cells cut wider than the rest, by exactly what the words need.
`ZOOM_SECTIONS` is `ZOOM_CHOICES` cut into three and a test holds the two in
step, since a choice in no section could never be pressed. The menu of copies
is a list rather than a grid, each item with its key printed beside it: the
items are things done rather than states to be in, and what a reader wants
from one is the key that would have done it without the menu.

The menu of other applications is the one whose contents this program does not
know: what is on it is whatever the desktop has installed, so an item is asked
for by its place in the list rather than by a `Control` naming a choice, and
there is no key beside any of them because there is nothing for a key table to
have bound. Its items are also the only ones sized to the words on them —
`TextWrapMode::Extend` — since nothing here chose how long an application's
name would be, and a name left to wrap in a popup that opened at the width of
the button below it comes out a letter to a line.

The panels are opaque, and the image is drawn in the `Viewport` they leave
rather than behind them: zoom, fit, pan limits and the wheel's anchor are all
measured against that rectangle. It is derived per frame from the window and
whether the panels are showing, never stored, so `` ` `` re-fits a fitted
image without anything having to notice that it should.


## Opening in another application

The button under the copy button hands the file on screen to something else,
and the menu it opens is read out of the desktop's own database rather than
guessed at: `src/openers.rs`. Every installed program ships a desktop entry
naming the MIME types it opens, `update-desktop-database` indexes those into a
`mimeinfo.cache` beside them, and the user's `mimeapps.list` says which is the
default and what associations they have added or removed by hand. Those three
files are the whole answer, and it is the same one a file manager's "Open
With" shows, because there is nowhere else it lives. Nothing is shelled out
to: `xdg-open` knows only the default and could not fill a menu, and `gio`
would be a runtime dependency on a package the user may not have.

Which file is which type is decided by the extension — `MIME_TYPES`, one entry
per extension the decoders read, listing every name the format is registered
under so that a viewer claiming `image/x-bmp` and one claiming `image/bmp` are
both found. That the decoders here sniff their way past a misleading name is a
courtesy this table cannot pass on: the desktop's database is keyed by name,
so a file whose extension lies about it is one no other program will recognize
either. A test holds the table against `decode::supported_extensions`, since a
format added to the decoders and not to it would be one this menu was silently
empty for.

An entry is left off the menu when pressing it could not work: it is not an
application, it has been deleted by a `Hidden` entry standing in front of it,
it wants a terminal there is none of here, or its program is not installed —
`TryExec`, and the first word of `Exec`, both checked against `PATH`.
`NoDisplay` is deliberately *not* one of those reasons: the specification
gives it for exactly the program that wants to be handed files without
appearing in the applications menu, which is this list and not that menu.
`gamut`'s own entry is left off too. What remains is sorted with the desktop's
default first and the rest by name, and any name two of them share is given
the entry's own id after it, since one entry to open a file and another to
open its directory are commonly both called the same thing.

Starting one is `Exec` split the way a shell would split it — quotes and
backslashes, and nothing expanded afterwards, because nothing goes through a
shell — with the field codes resolved against the file: `%f` and `%F` take the
path, `%u` and `%U` the `file:` URI that `clipboard::file_uri` writes, and an
entry that asked for no file at all is given it on the end. The argument is
built as an `OsString`, so a filename that is not valid UTF-8 reaches the
program as the bytes the file system holds. The child is put in a process
group of its own: a viewer started from a terminal and then closed would
otherwise take everything it had opened down with it, and a thread parked in
`wait` collects the child rather than leaving a zombie, while letting it be
adopted and go on running if this window leaves first.

The list is read once per file, in `App::apply`, beside the file's other
facts. It is a handful of small files and a millisecond or two — nothing
beside decoding a picture, and far too much to do sixty times a second — so a
program installed while the window is open joins the menu at the next file
rather than the next frame. Where it comes back empty the button is drawn
dead and says so, rather than being left out: a control that comes and goes
with the file on screen is one that has to be found again, and a missing
button could not have explained itself.

