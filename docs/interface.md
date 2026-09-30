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
`MAX_WINDOW_FRACTION` of that monitor's room — three quarters of its width
and 85% of its height, more of the height since a portrait picture on a
screen wider than it is tall would otherwise open small — and takes the largest of those
answers that fits on *every* monitor. A window sized that way cannot overrun
whichever screen it lands on. Where none of them fits everywhere — a monitor
smaller than `MIN_WINDOW`, say — the smallest answer is taken as the least bad
of them.

The whole calculation is in logical pixels. The image's own pixels are
physical, so a monitor's scale converts them; the panel constants are logical
already. Asking in physical pixels does not work, because winit's Wayland
backend converts the size a window is created with at a scale of `1.0` — the
surface has none until the compositor configures it — so physical pixels are
taken as logical ones and the window opens `scale` times too large, well past
the screen on a 4K monitor at 2×.

Where that scale comes from matters. What winit reports for a monitor is the
`wl_output`'s own scale, which the protocol makes an integer: a compositor
running 1.6 says 2 there, and the true fractional scale reaches a window only
through `wp_fractional_scale_v1`, once it has a surface — after its size was
asked for. Sized from the integer, an 800-pixel picture on such a monitor is
taken as 400 logical pixels, which the compositor maps at 1.6 to 640 device
pixels: a window that opens at 80%. So `monitor/wayland.rs`, which already holds a
connection of its own for each monitor's mode ([color](color.md)), also reads
each output's `xdg_output` logical size, and `Monitor::measured` in
`app::window` takes the room from that and the scale from its ratio to the
output's mode — the scale the compositor is actually running, whatever it was
configured as, since a compositor that adjusts a scale to make the logical
size whole reports the adjusted result. The output's mode is turned to match
its transform first, or a monitor on its side would have one axis measured
against the other. `Monitor::reported`, from winit's account, is the fallback
under a compositor without `xdg_output` or off Wayland, where the error goes
the safe way: a little under 100% rather than overrunning.

A window also opens no smaller than one the interface itself fits in.
`ui::PANELS_ROOM` is the content area the histogram and the information column
need together — the strip's width, the histogram at its tallest, the gap, and
the least column the panel will show — and `PANELS_WINDOW` is that plus the chrome
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
at on the same quarter-second cadence as the file and the palette, by a thread
of its own (`clipboard::watch`) since one look is a round trip to the
compositor, and the button goes with the bars when the interface is hidden. A
button that did nothing when pressed would be worse than no button, and one
that comes and goes says what the clipboard holds without being asked.

`--paste` is the same paste asked for before there is a window. `main::run`
asks the clipboard what it offers, reserves the file through `pasted::reserve`
as `App::paste` would, and puts it at the head of the list; the opening
request then carries `Source::Clipboard` through `Files::open_first`, so the
loader fetches the bytes into the file and reads it back exactly as it does
for `Ctrl+V`, and the reply arrives through the same `App::apply`. What is
different is what start-up can check: `cli::first_readable` probes headers to
turn a bad path into a plain command-line error, and the paste's file is
still empty at that point, so with a paste the probe is skipped and the
window opens at the default size rather than the picture's. A paste that
never arrives is walked past like a file that fails to decode, since the
opening request is a walk — which is also why the paste goes first: with
nothing on screen yet there is no neighbor to sit beside, and the user asked
for the clipboard's picture rather than the first path. `Files::open_first`
marks it adopted so that a relist keeps it, as `Files::adopt` does for a
paste made later.

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
line at a steady rate, and whatever a zoom is anchored on stays put from the
first frame to the last. The views along the line are not held within the
pan limit as a settled view is: the limit is `max(linear, 0)` in these
coordinates, bending where the image stops overflowing the viewport, so the
line from a fit to a zoom on a detail crosses it — for a few frames the
image overflows one edge while still short of the other. `View::at` marks
the view it makes as moving, and `placement` shows it where the line puts it;
clamped, the move would pin the picture against the edge and then let it
catch up, which is a bend. Only the ends are clamped, and the end is what
the move lands on.

Every zoom has an anchor, a window point that stays over the same detail
through it: `View::set_zoom_at` is the one zoom, and the steps, the number
row, the zoom menu, the double-click, the wheel and the actual size `Space`
cycles to all go through it. `App::zoom_anchor` chooses the point: the
pointer while it is over the picture — `pointer_pixel` says so, on the same
reading the bar's readout uses, so a pointer over a panel or in the margin
beside the picture does not count — and the viewport's center otherwise. A
zoom asked for from the keyboard with the pointer resting on a detail is a
zoom into that detail, which is what the hand on the mouse was saying; from
the zoom menu the pointer is on the menu, and the middle of the window is
what stays. Asked for the zoom the view is already at, `set_zoom_at` puts
the detail under the anchor in the middle instead — `1` at actual size goes
to the detail rather than doing nothing — as a move, being asked for by
name. The double-click is not that: its default is `zoom.100.toggle`, whose
`View::toggle_zoom` goes to actual size about the anchor from anywhere but
actual size itself, and from there to the whole image, so that in and out
again is two double-clicks on the same spot. Actual size is judged by the
view held rather than the view shown, so a second double-click during the
move the first began still goes back out, and by a zoom set by hand, so a
fit that comes out at 100% still goes in. The anchor cannot always be honored: an axis the image does not
overflow stays centered, and a view against an edge stops there, by the same
`clamp_pan` a drag is held by. The two fits have no anchor — a fit with the
view left off to one side would show a corner of the image it had just been
asked to fit — so `set_fit` centers. Apart from the copying chords and `Ctrl` with an
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
`--tone-map neutral`, `--window full`, `--exposure -1.5`, `--output hdr`,
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

The map is also a way of saying where to look. The hand on it puts the point
under it in the middle of the window, at the zoom the view has: from the
frame the button goes down, so a press moves the view at once, and on every
frame after that until it comes up, so holding on and moving keeps the
marked-out part under the hand. It comes back from the pass as
`Command::Center`, carrying the image pixel the hand is on, and
`View::center_on` moves the pan there — clamped as any pan is, so a press by
an edge stops with the edge at the window's, and an axis the image does not
overflow stays centered. Not animated, on the rule the rest of the view
follows: this is the hand on the view, as a drag on the picture is, and it
lands where it is put. The press is read with `is_pointer_button_down_on`
rather than `clicked` or `dragged`, both of which wait — the one for the
button to come up, the other for the pointer to travel far enough that the
toolkit is sure it is not a click — and a map that answered only then would
feel stuck to the hand. The thumbnail is life size at most, so on a small
image the map is a few pixels and a press on it coarse — which is what the
map is for a small image anyway.

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
the toolkit. A file of frames or pages brings a fifth with it, the transport
bar, above the bottom bar and nested between the strips as the picture is,
so its controls sit under the picture they act on and the strips run down to
the bottom bar either way. The [file list](filmstrip.md) is a sixth,
down the left edge of the window under the top bar and running to the
window's foot, with the left strip and the bottom bar starting at its
right edge: the list is a column of its own, and the controls that are
about the picture sit beside the picture. Both
are part of the same derivation, from the window size and `chrome::Parts`
— which of the two is up — so the picture is fitted beside and above them
on the first frame either is up rather than a frame later, and `ui::show`
derives the same `Parts` from what it was handed, so a frame given the
list's rows is a frame laid out with the list. It holds the one-frame-back and
one-frame-on buttons as a pair, with the play button between them for an
animation, then a readout — which frame of how many, and where that is in
time — and, for an animation, a timeline in whatever width is left. The
timeline is laid out in time rather than in frames, so a frame shown for a
second takes ten times the track of one shown for a tenth and a drag along it
runs at the speed the animation plays; its handle sits at the middle of the
span of the frame on screen, so a press on the handle is a press on that
frame. A file of pages has no clock and so no play button and no timeline:
the steps and the count are the whole of its bar. What the bar shows comes
down as `Transport`, and a press goes back as a `Command` like every other;
`docs/animation.md` is where the clock behind it is explained. The
top bar carries which file it is — its place in the list, in front of its
name, so that the count is always in the same place whatever the name is — and
what the image is: its size, its pixels, its color space, all fixed for as
long as the file is on screen. The name is the only thing in the window set
bold, and the only thing drawn in the ink the theme keeps for it; the count in
front of it is set like the facts at the other end of the bar, since it is one
of them. Picking out two things picks out neither, and what a reader wants
from that bar at a glance is the name. The facts are also what yields: they
are given what the near end of the bar leaves with the name written in full
(`status::head_width`), cut by whole segments from the color space back to
the size (`status::fit_segments`), and left out altogether when even the size
has no room, so a narrow window names the file rather than describing one it
cannot name. The bottom bar carries what changes: what is
under the pointer, and what the view is doing to the image, the last of which
says nothing at all while nothing is being done. The pointer's end of it is a readout of one pixel —
where it is, the components the file holds there in the file's own units, the
values the display window maps them to, and a swatch of the color they come
out as. The two numbers answer different questions, which is why both are
there: the stored one is the measurement, the mapped one is why it looks the
way it does. Saying what the screen is showing means running the display
transform on the CPU: `ToneMap::apply` in `image/display/tone_map.rs` mirrors the
shader function of the same name, and `Colormap::color` is what
`render/image_layer.rs` writes the shader's ramp texture from — a swatch
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
whether the grid is drawn and how far apart its lines are — then the loupe
toggle, and after it the button at the head of the pixel readout. A grid laid
over the picture is something being done to it rather than a fact about the
file, so it belongs in the bar that carries what is being done; it is in a bar
at all rather than in a side strip because it has a spacing to read out, and
the strips are a button wide, which is too narrow for words. The loupe is the
same kind of thing as the grid — a way of looking at the picture — and sits
beside it for that reason, lit while the loupe is on by either means: the
button says what is in force, and the secondary button holding the loupe up
puts it in force as surely as the toggle does. Its mark leads the bar and its reading
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
fixed height besides: its plot gives a bin to the logical pixel, and the
three rows under it are every file's — see [the histogram
panel](histogram.md) — so there is nothing in either to give. A content area
smaller than one of them
gets no panel rather than one drawn over the picture it is about and off the
edge of the window. `ui::room` asks the question for both at once, because
they are stacked: the histogram takes the top of the strip, and what it takes
is height the column below it does not have, so a window can have room for the
column alone and none for it under an open plot. What it takes is settled by
`ui::histogram_shown`, which asks whether the plot is on screen rather than
whether its toggle is on — a window too short for the plot is not one the
column has to start below — and hands `info::panel` the rectangle it took, so
that the two panels cannot disagree about where the column begins.

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


## Turning the picture

A turn the user asks for is not applied to the pixels. The `DecodedImage`
and the texture it was uploaded to stay as the file holds them — the stored
space — and `Current::turn`, an `image::orient::Turn` of quarter turns
clockwise, is read through at the two places a pixel is fetched: the vertex
shader in `shaders/image.wgsl`, whose four corners read the stored corners
the turn brought there, and `Current::sample` on the CPU, which maps through
`Turn::stored` before `DecodedImage::sample`. Everything else is in the
turned space: `Current::size` and `Current::pixels`, and so the view, the
placement, the region, the pointer's coordinate and the minimap. The
information panel keeps the file's stored size, since it describes the
file.

Re-uploading a turned copy would have been simpler to get right — every
reader would see turned pixels without knowing — but it copies every sample
on the main thread, column by column, for a 16-bit RGB picture of twenty-four
megapixels well over a hundred megabytes, and the window would stop for it
at every press. It would also have to refuse an animation, whose player
writes each frame into the texture as stored. Read through, a turn costs a
uniform, frames and pages arrive under it, and it is kept per file in
`app::kept::Settings` as the display is.

The discipline is that nothing reads the stored size or samples the stored
picture on the picture's behalf except those two places.
`render::filter_tests::a_turned_picture_draws_as_the_picture_turned`
compares the shader's reading with `orient::apply` done on the CPU, and
`a_turned_picture_is_averaged_along_its_own_axes` the per-axis density
`params_for` swaps under a quarter turn; `encode::displayed`, which walks
the picture for a copy or an export, reads through `Turn::stored` too, and
`encode::tests::a_turned_copy_is_the_turned_picture_copied` holds it to the
same `orient::apply`. The pan is measured from the picture's center, which a
turn leaves where it is, so `View::turn` turns the pan with the picture and
the detail at the middle of the window stays there; a region is turned by
`Region::turned` with the pixels it marks out.

## Stepping to another file

A step changes two kinds of thing, at two different moments. What says
where in the list the key has gone moves at once: the count, the file
list's highlight and its scroll, the name in the bar and the window's
title. They read `Files::target` — the file being read, or the file on
screen where nothing is — and `App::send` owes the frame that shows them.
What reads the picture on screen moves with the picture: the pixels, the
pointer's readout, the zoom, the region, the loupe, the minimap. What
describes the file goes at the key, since it would sit beside a name that
is no longer its own: the size, pixels and color space at the top bar's
end, the words at the bottom bar's end saying what is being done to the
picture, and the camera's switch. The transport bar becomes the arriving
file's at the key, from the frames or pages its header said
(`App::arriving_transport`), at the frame or page it will open on, with
the room it takes: the picture on screen gives up or takes back a bar's
height for the moment it is still up, rather than the bar describing a
file the name has left. A file whose header is not read yet has no bar
until it arrives. A read that fails hands the target back to the file on screen, so
the readouts go back by themselves, and `App::deliver` puts the title back
with them. `app::tests::the_list_readouts_follow_the_key_and_go_back_when_a_read_fails`
holds both halves.

Between the two moments the bar names one file and the screen shows
another, so what acts on "the file" — the copies, rename, remove, delete,
export, opening it elsewhere, the turn — is dead while `App::arriving` says a file is
on its way in (`Reasons::arriving`, `tooltip::STILL_OPENING`), and so are the arriving
file's own transport bar's steps, play and timeline, there being nothing of
it decoded to move through. Acting on the
file on screen would act on one the bar no longer names; acting on the one
named would act on a file that is not open yet.

Most reads land within a few frames, and nothing else happens. One that
takes `files::SLOW_READ` is announced, and the picture leaves the screen:
the thumbnail stands in for it where it can, and the panels wait. That is
also one moment rather than two — a panel describing a picture that is no
longer shown would be wrong — and it is not the key's moment, because a
thumbnail enlarged for a frame or two before a quick file arrives is a
blurred flash, and two pictures of one size flipped to compare them would
flash through a blurred third.

The exception is a read known to be slow before it starts. `App::read_rates`
keeps, for each decoder, how long the last file it read took to arrive over
its pixels; `App::predicted_slow` scales that by the pixels the file's
header says it has (`Chooser::facts_of`), and a read expected to take
`SLOW_READ` is announced as it is sent (`Files::announce_now`). A folder of
large raws flips straight to thumbnails; a folder of small PNGs never shows
one; a large file among small ones of its kind is judged by its own size.
A walk carries the announcement on from each read it replaces, so a held
key keeps the thumbnails up however fast it goes.

### The thumbnail

Where the thumbnail thread has already said the file's size and its
thumbnail is among the textures the screen holds (`Thumbs::get`),
`App::standin` puts the thumbnail up in the picture's place: a
`ui::Standin`, painted by `ui/standin.rs` in the picture's own panel, under
everything that floats. The toast saying the file is loading goes up with
it: a blurred picture with nothing to say why reads as a fault in the
viewer rather than a file on its way. So that there is a thumbnail, `App::arrived_in`
asks the thumbnail thread for the files either side of each file that
arrives, ahead of the rest of the session, where the screen does not hold
them already.

It lands where the picture will. `arriving_view` in `app/mod.rs` is the one
reading of what view a file arrives with — the view on screen for a file of
the same size, the view it was left in for one coming back, the view rescaled for
another rendering of the same file, a fresh fit otherwise — and both `App::apply` and `App::standin` go through it, with the
turn the file was left in, so that the picture replaces the thumbnail
without anything moving. `app::tests::a_slow_read_stands_the_thumbnail_where_the_picture_lands_and_the_panels_wait`
holds the two to each other. The thumbnail is of the picture as the file
holds it, like the texture, and is read through the turn at its corners
as the vertex shader reads the texture.

It is the interface's to draw, in the thumbnail's own sRGB, rather than the
image layer's: it is a stand-in, and the window, tone curve and false color
the picture will be shown under do not apply to it. While it is up,
`Scene::picture` keeps the image layer from drawing the picture being
stepped away from, and what reads or marks up the picture on screen — the
pointer's pixel, the grid, the region, the loupe, the minimap — stays off,
since what it would read or mark is not what is shown. The file on screen
read again gets no stand-in: the picture already up is a better picture of
it than any thumbnail.

### The panels

The histogram and information panels wait on the same announcement,
thumbnail or none (`App::replacing`, handed over as `FrameInput::waiting`):
each keeps its place and size and shows `panel::waiting`, its background and
a spinner, until the file arrives. What they said was about the picture
leaving, and their controls act on it; a panel that emptied or shrank
instead would move the one under it, only for both to move back a moment
later. The spinner asks egui for a pass on every pass, which `Gui` folds
into the loop's deadline, so the window redraws continuously for as long as
the wait lasts and not after.

## The camera's JPEG

A raw holds two pictures, the frame developed from the sensor's counts and
the JPEG the camera rendered from them, and `image::decode::Rendering` names
which one a read asks for. It is one preference for every raw, `App::rendering`,
kept between runs in `settings::State::camera_jpeg`, rather than something of
each file's: what is being asked is how raws are to be looked at, and a
preference that held per file would have the same key mean different things
from one file to the next.

`App::send` fills every request's `rendering` from the preference, whatever
the reason for the read, and `loader::decode_rendering` makes the choice: the
camera's JPEG through `decode::preview_timed` where it was asked for and the
file has one, the developed frame through `decode::load_timed` otherwise.
A raw without a JPEG, or with one that will not decode, is read as developed
and the read succeeds, so a step onto one is not a step onto a broken file.
`Ready` carries back what was read and `CameraJpeg`, whether the file has one,
and both land on `Current`, which is what the bottom bar's switch
(`Pass::camera_switch`), the tooltip (`Namer::tooltip`, from `Namer::camera`)
and `Conditions::camera_jpeg` read. When the JPEG was asked for and the file
has none, `App::apply` says so in a toast.

The switch is left out, not drawn dead, on a file with no JPEG. The headroom
switch beside it is always drawn, because the surface is a question about
every picture; this one is about a kind of file most pictures are not, and a
dead button on every PNG would be a button about something the file is not.
The key still needs a reason to be refused, so `ui::tooltip::disabled` has
one, `NO_CAMERA_JPEG`, which only a press of `v` ever reaches.

A press flips the preference and asks for the file on screen again as
`Reload::Rendering`, which `arrival` names `Arrival::Rerendered` at any size:
the view is carried across by `View::rescale`, which scales the pan and the
zoom by the ratio of the two sizes so the same detail stays under the center
of the window at the same size on screen, whether the JPEG is the frame's
size or a quarter of it. The display starts over from
`Display::for_image_with`, since an exposure set on linear sensor counts
means nothing to an 8-bit JPEG, and `kept::Settings::rendering` stops a file
left in one rendering from having what it was left in put back onto the
other. The region is cleared: the JPEG is other pixels, and scaling a
rectangle onto a smaller picture could only be approximate. The file list
keeps the file's own size, so `App::apply` learns the chooser's `Facts` from
the developed size and not the JPEG's.

`Files::rerender` refuses while a read is in flight, as a reload does,
because that read may be a step and a request would cancel it. The reply
carries the rendering it was asked for on `Opened::rendering`, and
`App::follow_rendering` asks again once it lands if the preference has moved
in the meantime.

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

What the application holds is a `Marking`, in `app/region.rs`: a
`Selection` — off, asked for, or drawn — and, during a drag, what was
taken hold of, the region as it stood at the press, and where the press was
in image pixels. Every frame of the drag remakes the region from those
three and the hand's place, rather than from the frame before, so a drag
that goes off the picture and comes back has nothing accumulated in it. The
interface reads all of this from `FrameInput` and never writes it: what it
hands back is `Command::Grab`, `Pull` and `Release`, and `App::act` hands
those to `Marking`'s `grab`, `pull` and `release`, which are tested with no
picture and no window.

The drag is classified where the button went down, not where the pointer is
when the toolkit calls it a drag. egui defers the decision until the pointer
has moved six points or the button has been held most of a second, and by
then the pointer is off the press — so `Pass::region_gestures` reads
`press_origin` and tests that against the handles. With a region asked for,
any drag draws one; with a region on screen, a drag from a handle pulls it —
or moves the whole of it, from the handle at its middle — and a drag from
inside it moves it only where the drag's slot is `move-region`, which is
`Shift` with the left button by default. A drag from anywhere else, and from
inside without the key, is what the button's plain slot says — the view's,
by default, as it always was (see [keys and gestures](keymap.md#gestures)): a region is drawn to be looked at, and one that covers the window
would otherwise pin the view under it. Which the pointer is about to do is in
the cursor — the four-way arrow on the middle handle, and inside only while
the key is down — and egui repaints on a modifier change, so the cursor
follows the key. The arrow is asked for as `AllScroll`, not `Move`: Adwaita,
the cursor theme a desktop with none set falls back to, draws `move` as the
plain arrow. The hand's place goes back to the application in
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
drag on a handle is read off. The nine handles are placed on the device's
grid through `icon::Grid`, like every other thin thing over the picture, and
hit-tested with a little reach past their edges; a corner is asked before
the edges it overlaps on a region drawn small, since it moves two edges
where they move one, and the middle is asked last, since the whole region
can be taken hold of from anywhere inside it as well. `Grip::Middle` and
`Grip::Inside` do the same thing to the region and are kept apart because
they do not do the same thing to the pointer: the one is a hold whenever it
is under the hand, the other only with the key. Which handle the pointer
rests on goes back each pass as `Command::OverGrip`, a pass late like
`OverImage`; that is what the region's words are written for, and nothing
else reads it.

One handle is the *current* one, `App::handle`, and that is what the arrows
move: the middle — the whole region — for a region just drawn, and after
that whichever handle was last clicked or dragged. A drag says so through
the `Command::Grab` it already sends; a click, which egui never turns into
a drag, is read off `clicked_by` in `Pass::region_gestures` and sent as
`Command::Handle`. A move of the whole by its inside leaves the current
handle alone: it is not a handle, and the arrows should go on moving what
they were moving. A handle that stays chosen until another is chosen can be
worked on with the hand anywhere, where arrows that consulted the handle
under the pointer would make precise sizing a matter of holding the pointer
still on an eight-pixel square while pressing keys. The current
handle is drawn apart from the rest — filled in `text_bright`, the ink that
leads, and edged in the accent, where the others are the accent edged in the
bars' ground — so that what the arrows will move is always in view. An arrow
along the current edge — Up on the right edge's handle — moves the region
rather than doing nothing, so no key is dead while a region is up. `Ctrl`
with an arrow grows that side, and `Ctrl+Shift` with an arrow shrinks it
that way, pulling in the side opposite — `Region::grown` and
`Region::shrunk`, the second stopping a pixel short of the far edge so a
region cannot be keyed out of existence. `Shift` with an arrow is not taken
by the region at all: a region moves by the pixel already, so a fine step
would duplicate the plain one, and the picture under the region still wants
placing to the pixel. None of it is animated: a region moves a pixel at a
time, and a pixel has nothing to animate.

The region wears its measurements while the pointer is on it: its size
under the handle at its middle — over it where the region runs off the foot
of the window, and on it where the region is too short for either, the size
being worth more than a handle the region has another way of being moved
by — and each edge's coordinate inside the mark in the middle of that
edge. They come and go with the hand rather than with a clock, since the
hand is what says which region is being worked on, and a region left on the
picture keeps only its outline, which is the thing it is for. `Marking::over`
is the reading, and it is `Marking::grip` — the hit test the interface
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
follow the window. With a region up the key runs a cycle of five rather than
its three — the region fitted and filled, then the picture fitted, filled
and at actual size — since there are two things on screen to frame, and the
region, being the thing under the hand, goes first. Where the cycle has got to is
`App::framing`, an `input::Framing`, kept apart from the view for the same
reason as the zoom; `App::select` puts it back to the start, so that a region
just drawn or moved is what the next press shows whatever the press before it
showed.

`Esc` takes the region off after a message and before quitting: a message is
about what was just done, the region is what was being done to, and the one
that stops being news first goes first. Stepping to another file takes it
off, and so does the file coming back a different size — the pixels it
marked out are no longer the pixels — while a reload at the same size keeps
it, being the same picture read again. The key is `x` rather than `k`, which
the histogram's planes toggle already holds in the j/k/l run.

## The zoom box

Holding `Space` — whatever key `zoom.fit` is bound to — and dragging a box
on the picture zooms to the box: a drag of the primary button started while
the key is down is `Grab::Zoom`, whatever the selection, as is any drag
whose slot is `zoom-box`, and
goes through the same `Grab`, `Pull` and `Release` commands as a drag on the
region does. The box in progress is `App::zoom_box`, a `Region` made by
`Region::from_corners` exactly as a new region is, but held apart from the
selection: it is painted by `ui::region::show_zoom_box` — an outline with a
wash of the accent inside, and none of the region's handles or words, since it
is gone the moment the drag lets go — and `App::release` takes it and moves
the view to it through `View::fit_region`, as a move, the way any zoom asked
for by name is made.

The key is answered on its way up, not down. Down would fit the picture
before the drag began, and the hand about to draw a box would find the
picture moving under it; so a press only records that the key is held
(`Pointer::fit_key`, an `input::FitKey` holding the physical key it was
pressed on), a drag begun while it is held marks it
as spent, and the release fits only if nothing was drawn. A tap costs the
fit the length of the tap, which is not noticed. The key's repeats arrive as
presses while it is already held, and are not answered: a held key that
toggled the fit as fast as the keyboard repeated would be no use held. The
release is read by the physical key rather than through the table, so that
a chord pressed while the key is down cannot leave it held for good, and
`WindowEvent::Focused(false)` lets go of it too, since a key held as the
focus goes is released somewhere else. `Esc` part way through the drag drops
the box before it takes anything else off: egui aborts the drag on the same
key, and the `Release` it sends on the next pass has to find nothing to zoom
to.

## The loupe

The loupe is two circles: the glass, `ui::loupe::GLASS_RADIUS` logical
pixels across whatever the magnification, and the eye around the pointer,
the glass's radius over the magnification, so that the glass shows exactly
what the eye rings. The magnification is `Panels::loupe_magnification`, one
of `ui::loupe::MAGNIFICATIONS` — 2, 4, 8 and 16 — starting at 4. The wheel
over the picture with the secondary button down is the loupe's rather than
the view's — its default slot, `gesture.image.right+wheel` — so `App::wheel`
hands it to `App::magnify`, which steps the magnification a notch at a time, stopping at either end, with a trackpad's fractions of a notch adding
up in `Pointer::magnifying` until there is one; `Shift+L` is
`Action::CycleMagnification`, which goes round instead, through
`ui::loupe::cycle`, since a key pressed again and again wants to reach every
setting. The glass keeps its size
because it is the thing beside the pointer, and a thing that grew and shrank
with a setting would be a different thing to find each time; what the
setting changes is how much of the picture fits in it, which is the eye's
business. The button reads the magnification out beside its mark while the
loupe is up, through the same `reading_toggle` the grid's spacing is read
out through. `Control::Loupe`, the toggle beside
the grid's in the bottom bar, which `l` presses too, keeps it up; a button held on the picture whose hold slot is the loupe — the
secondary, by default — puts it up for as long as it is held, whatever the
toggle says. Both are read into `Panels::show_loupe` and `Pointer::held`, and
`App::loupe` is the one answer to whether it is up: one of those, and a
pixel under the pointer — the same `pointer_pixel` reading the bar's readout
is made from, so the loupe is up exactly when there is a pixel to magnify,
and goes when the pointer crosses onto a panel or off the picture's edge.
Through a drag on the picture — a pan, the region's or the zoom box's — it
follows the hand: the pointer the loupe follows stands still while the
toolkit holds the drag, so `Pass::picture` reports the pointer's place on
every pass of one as `Command::Dragging`, and `App::act` moves the pointer
by it, as `Command::Held` does for the other buttons.

Where the circles go is `ui::loupe::place`, a pure function of the pointer
and the content area, worked out by the application once per frame and
handed to the interface in `FrameInput::loupe` and to the renderer as a
`render::Glass`. The glass goes up and to the right of the eye, where the
hand is least likely to cover it, and the other way on whichever axis that
would run it off the content area; it is then held inside the area, which in
a window too small to hold it beside the eye puts it over the eye rather
than half under a panel. The interface draws only the two rings, in the theme's
`inset_edge` — the minimap's border's ink, the outline for a picture the
image layer draws inside the picture, rather than the accent, which says
what is switched on — on the picture's own painter as the region is drawn,
so the picture under the loupe keeps the pointer; and the grid, which is the view's spacing rather than the
glass's, is broken around the glass's circle — each hairline cut over the
chord it makes of the circle, by `grid::chord` — as it is broken around the
minimap's thumbnail. The minimap is over the whole loupe: the thumbnail is
drawn after the glass in the image layer, and the rings are clipped around
the thumbnail's rectangle — clipped rather than stacked under the minimap's
area, since the thumbnail is not egui's and nothing egui puts over the rings
would cover it. The map stays readable in its corner, and the loupe is the
thing that moves.

The glass itself is the image layer's: a third quad in its pass, over the
view and under the minimap's thumbnail, drawn from the same texture through
the same shader as the view, so that the window, the false color, the tone map,
the lift and the turn all reach it for nothing — the same reasoning as the
[minimap](#minimap)'s thumbnail. `ui::loupe::glass` places it: the view's
placement magnified, with the image point under the eye landing at the
glass's center, and the circle in physical pixels. Two things are different
about this quad. Its corners are the circle's square rather than the
magnified image, which runs far past the glass, and the shader computes each
fragment's place in the picture from the image's own placement
(`Params::picture`) and cuts the square to the circle (`Params::clip`). And
it is drawn with no blending, through `ImageLayer::replacing`, so that what
is inside the circle is the glass and nothing else: past the picture's edge
the glass writes nothing, which the compositor shows as the backdrop, where
source-over blending would have left the view showing through under it, and
a translucent picture has the checkerboard under it rather than the view.
The circle's edge is the one place replacing is wrong: a pixel the edge
crosses, feathered, would be the glass blended over the backdrop rather than
over the view, and on a light theme that drew a light hairline around the
glass just inside the ring. So the glass is two quads sharing one circle
(`image_layer::Cut`): the disc to a pixel short of the edge, hard-cut and
replacing, and the last pixel as a band, feathered and blending, drawn after
it. Past the picture's edge that band leaves the view showing through, so
the compositor, which knows the circle and the picture's rectangle already
for its checkerboard, shows the backdrop alone inside the circle and outside
the rectangle. The rings are drawn half a stroke in from their circles,
since egui lays a circle's stroke outside its radius, so that the feathered
pixel runs down the middle of the ring. The compositor's
checkerboard follows the same cut — the glass is its third region, and the
one it tests against a circle as well as a rectangle — so transparency reads
the same inside the loupe as outside it.

The buttons besides the primary cannot be read from winit. egui-winit consumes a button
event wherever egui wants the pointer, which over the picture's own panel is
everywhere, and consumes the pointer's moves while egui holds a button down.
So `Pass::picture` reads the button off the picture's response —
`is_pointer_button_down_on` with `button_down`, from the press on rather
than from the toolkit's later decision that the press became a drag — and
says which on every pass as `Command::Held`, carrying the pointer's place
in physical pixels while the button is down, for the same reason
`Command::Pull` carries the hand's: the application's own pointer stands
still meanwhile. `App::act` takes the button and the pointer from it, one
frame late as `Command::OverImage` is, which is a frame the press was being
painted for anyway. A drag with a button is a pan only where
its drag slot says `pan`, which by default is the primary's alone.

The loupe follows the pointer itself, not the pixel under it. A move over
the picture ordinarily owes a frame only when the pixel under the pointer
changes; with the loupe up, `CursorMoved` asks `App::loupe` before and after
and owes one whenever the answer moved, so the glass tracks the hand at
every zoom rather than stepping from pixel to pixel at a high one.

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
to. Two of the picture's facts come out of the EXIF block rather than the
decoder: the turn its orientation tag asks for, which is why the resolution
can be the stored picture's on its side, and a TIFF's compression — only a
TIFF's, since a raw's first directory describes the preview in front of it
and every other container's block has no pixels of its own.

The rest is its EXIF, read by `src/image/exif.rs` on the loader thread beside
the decode, because it is one more parse of a file somebody else chose the
bytes of and that is the thread with the panic guard around it. What comes
back is already words, and already grouped: `Camera`, what took the picture
whatever it was taken of — the body, the lens, the owner and the serial
numbers — then `Exposure`, how this picture was taken — when, the exposure
and how it was decided, the focal length with its equivalent and any digital
zoom, the metering, the white balance and the color temperature it was set
for, the flash, and whether the picture was merged from several frames —
then where it was taken, the coordinates in degrees a map will take, and
last whatever somebody wrote in words. Nothing else is shown field by
field. Once those are read out of the block, what is left is how the file
is laid out —
strip offsets, bits per sample, a DNG's calibration matrices — and how the
camera describes itself — the Exif version, APEX restatements of the
exposure already given — and a column of it buried the few fields worth
reading. A group that came to nothing is not carried at all, an empty heading
being a question about where the rest of it went. Each group is an `exif::Group`, and
`ui/info.rs` gives each its mark and its head in one exhaustive match, so a
group cannot reach the panel without them; the regions come last.

The coordinates are kept as numbers too, as `Exif::position`: the latitude
and longitude in signed degrees, south and west below zero, which is what a
map's address takes where the panel shows the hemisphere's letter. The
button after them at the head of the `Location` section opens
`settings::map_link` of the `open_map_link` setting — `{lat}` and `{lng}`
replaced by those numbers to six places — through `openers::browse`, which
is `xdg-open` on Linux and the workspace's `openURL` on a Mac. It is drawn
only where the file gave both numbers, so it is never dead. A setting that
does not name both is refused as it is read: a link with one of them fixed
would open the same place for every picture.

The words — the `About` section — are read from two blocks, because a file
keeps them in two. EXIF is what the camera wrote, and has a tag for a caption,
an artist and a copyright; XMP is what every program since has written — the title, the
keywords, the caption a cataloging program keeps — and a file that was only
ever given a title has an XMP packet and no EXIF block at all, which is a
file the EXIF reader alone had nothing to say about. `src/image/xmp.rs` finds
the packet in each container by walking its headers, the same way the EXIF
reader finds its block: an `APP1` segment in a JPEG, told from the EXIF one by
the namespace it opens with; an `iTXt` chunk in a PNG; the `XMP ` chunk of a
WebP; the `xml ` box of a JPEG XL container; a `mime` item in a HEIF, which
the container's own item tables lead to and so is asked of `libheif` through
`decode::heif::xmp` rather than found by hand; and tag 700 of a TIFF, which
the EXIF reader has already parsed and hands over. The packet is RDF/XML, so
it is parsed rather than searched — `roxmltree`, already in the tree under
`fontdb`, builds a document of it, and refuses one with a DTD, which is where
an XML parser's trouble with untrusted input lives — and comes back as
namespaced properties: a list's every item, a set of translations' default.
A sidecar — the picture's name with `.xmp` in place of its extension, or
after it, since darktable spells it the second way and the specification the
first — is read beside every file, whatever the container, and a property it
holds is taken over the packet's: a raw has no packet a program will write
to, so the sidecar is the only place its words are, and where a file has
both, the sidecar is the one written last. The rest of the packet's
properties stand, since a program that writes a sidecar writes only what it
was told. The sidecar is read whole, up to the same ceiling a packet is
believed to, and one that is not a packet is ignored rather than allowed to
empty the panel.
Which of those the panel shows is `exif.rs`'s `DESCRIBED` table, one row per
thing said in words, naming the EXIF tag and the XMP property that say it.
Where a file has both, the EXIF field is shown: it is the older of the two,
and a program that writes both writes them alike, so the choice rarely shows.
The Metadata Working Group's rules for which is newer are not applied: they
want the dates both blocks carry to be compared, and a panel that shows what
is actually in the file is better served by a fixed rule it can state. The
heading is "About" and not
"Description" because one of its rows is the description, and a heading that
shares a word with a row under it reads as a mistake; the row in turn is
"Caption", the word the programs that write the field use for it, and what it
holds — a sentence about the picture, not a description of the file.

For a raw, what LibRaw read of its header fills in whatever of `Camera`
and `Exposure` the EXIF left out, a row at a time and each where the EXIF
reader would have put it (`Exif::fill`): all of it for a CRW, which has no
EXIF. LibRaw also gives the color temperature, which EXIF has no field for.
The camera's as-shot multipliers balance a gray surface, which the sensor
therefore read as their inverse; LibRaw's matrix takes the sensor's values,
scaled by its own daylight multipliers, to sRGB, so that gray scaled that
way and put through it is the illuminant's color. From there it is CIE xy,
and McCamy's cubic gives the correlated color temperature, to the nearest
50 K. On a Canon R6 Mark II it comes within 2% of the temperature Canon's
own maker note records.

A lens is named by `LensModel`, with `LensMake` in front only where it is not
the camera's maker and the name does not already say it — the case of a
lens by another maker, which is where it tells the reader something. Where
there is no name, `LensSpecification`'s range stands in for one; the two are
never shown together, since the name nearly always carries the range.
The program that wrote the file (`Software`, `xmp:CreatorTool`) and when it
last did (`DateTime`) are said in words too, and are not in the table: a
camera writes both into every file, its firmware and the moment of the shot,
so they would head every photograph's words with two rows nobody wrote. The
section is headed as `Camera` is, by its title where
it has one, beside the mark the panel's own button wears.

One XMP property is read as the structure it is rather than as words: the
Metadata Working Group's regions (`mwg-rs:Regions`), which a cataloging
program writes for the faces, pets, barcodes and points of focus it found or
was told of. `xmp::Region` holds each as the packet wrote it — name, kind,
description, a barcode's value, a focus point's usage, Lightroom's
`Rotation`, and an area whose `x` and `y` are its center, in shares of the
sides unless the packet says pixels — reading a structure in any of the
three ways RDF allows one to be spelled, fields as attributes or as
elements. The guidelines put a region in the picture as stored: "Region
metadata is applied to the stored image. When applying a rotation by
applying Exif Orientation, the rotation must be applied to the regions as
well." EXIF's own `SubjectArea`, where the camera found the main subject,
is measured the same way — in pixels of the stored picture, before the
orientation — so the two are one thing to the panel: a
`metadata_region::MetadataRegion`, which is a label, a name, the other
things said about it already in words, and a `Shape` (a point, a circle or
a rectangle about a center) in the `Units` its source wrote it in, shares of
the sides or pixels of a stated size. Each source is turned into those by a
constructor of its own. `MetadataRegion::subject_area` reads `SubjectArea`
and `SubjectLocation`, its older one-point form; a location at the middle of
the area is the same subject said twice and is said once. `MetadataRegion::mwg`
reads the Metadata Working Group's list, and `MetadataRegion::microsoft`
Microsoft's people tags (`MP:RegionInfo`), which Windows Photo Gallery wrote
and digiKam writes beside every MWG face — so a tag with the same name as
an MWG region, and a rectangle within a hundredth of the picture's sides of
it, is left out as a repeat. Microsoft's rectangle is `x, y, w, h` from the
top left corner in shares of the sides (its documentation's prose says
height before width, and its own sample only adds up the other way), its
documentation spells the namespaces with `https` where every file written
has `http`, so both are read, and it says nothing of the orientation, so
the tags are taken to be measured as MWG's are. The account digest and ID
beside each name are not read. The subject comes first, being the camera's. `Exif` keeps the regions
as numbers beside the orientation tag, and the panel writes them out as it
draws, through `Exif::regions` and `MetadataRegion::placed`, which carries
each shape through the tag's turn and then the turn in force
(`orient::upright` is the tag's reading for a point, the inverse of the one
the pixels are fetched through) and scales it to the picture's size — so a
region reads as a marked region and the pointer's coordinate do, a top left
corner and a size in the picture as shown, and a picture made smaller since
it was marked is still marked in the right place. `AppliedToDimensions` is
read only to measure an area written in pixels. `Rotation` is not in the
specification and is written only by Lightroom, as zero, so it is shown only
when it is not. A file whose orientation came from somewhere other than an
EXIF block this reads — a JPEG XL's codestream — has its regions taken as
upright.

The section is a table: the subject — who
or what is in the region, or its kind where nothing names it — and the
region's top left corner and size, a circle's the square around it and a
point's no size, set right as numbers in a column are. `Placed` is the
shape in the picture as shown and unrounded, which the table writes out
whole and `mark_regions` in `ui/info.rs` draws: while the pointer is on a
row, its region is outlined on the picture with its subject on a pill
over it, and while it is on the heading, every region is. The
outline is dashed, as the heading's mark is, so it is not taken for the
region marked out by hand, which is solid and has handles. It is painted
on the layer the picture's own marks go on — the painter the panel's
`show` takes before it opens its area — and so goes under every panel,
this one included; the hover is known only as the panel is laid out, and
painting there rather than remembering it for the next frame keeps the
outline on the frame the pointer arrived in. A row copies as its five
cells in a line of CSV, and the heading as the whole table under a line of
its column heads, rather than as the two columns every other section
copies as: the section is a table on screen, and a region pasted anywhere
is wanted as its numbers.

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
corner and told plainly that there is no rectangle to quote. The panel
consults no coordinate-system register: EPSG:2056 is quoted as EPSG:2056,
beside whatever the file calls it.

The pointer's readout does consult one, because a latitude cannot be had
without it. `geo::Georeference` is built from the same tags beside the panel's
rows and carried on `Exif::georeference`: the tiepoint and scale, or the
matrix, as one affine map from raster to model — so a turned raster, which
the panel declines to give an extent, still places every pixel — and, where
the file names its system by an EPSG code, a `proj4rs::Proj` for that code and
one for WGS 84 to lift a model coordinate between them. The code is resolved
through the table `proj4rs`'s `crs-definitions` feature carries, which is a
proj4 string per code; its WKT half and the crate's default `aeqd` projection
are left off, the first being text this never reads and the second a
geodesic crate for a projection no GeoTIFF names by code. What the table
buys is breadth — every UTM zone, every state plane, every national grid, in
a few megabytes of strings — and what it costs is precision: a proj4 string
shifts datums by at most seven Helmert parameters, not by the NTv2 grids
PROJ proper would load, which is a meter or two where a grid would be
centimeters. For a readout under a pointer that is a pixel or less on
anything but a survey-grade orthophoto; a register that needed PROJ, sqlite
and a grid directory installed was not worth that difference.

Each pixel is read at its middle, which is half a pixel in from the tiepoint
where the file says its coordinates are corners and on it where they are
centers, and through `Turn::stored`, since the placement is of the raster as
stored and the pointer is in the picture as turned. A file whose system is
user-defined, or named by a code the table lacks, or whose code has an inverse
the crate does not implement, has no lift and offers only its own
coordinates; one in latitude and longitude already offers only those, read
straight or lifted from its own datum. `CoordinateFormat::offered` is the one
reading of that — the menu's rows, the key's step and the `When::Georeferenced`
and `When::Geographic` conditions all ask it — and `CoordinateFormat::shown`
is what makes the pixel the fallback without forgetting the choice, which is
held in `Panels` like the pixel format beside it.

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

Three things are decided rather than read. Numbers are rewritten to the
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

The block is not taken from the decode instead. The decoders that could give
one cheaply — WebP already reads it for the orientation, JPEG holds the file
whole — are the ones that cost nothing to re-read. The one that would benefit
cannot: the TIFF decoder has the directory parsed, but the crate behind it
will not follow the sub-directory pointers the exposure, the lens and the
coordinates live behind, so a camera TIFF would come back with less than a
second read gets, and its values would arrive as numbers needing a second
renderer to say what they mean.

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
the window by egui rather than withheld.

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

These menus of items are the ones with a name at their head — **Copy**,
**Open in…**, and **File**, the menu of the file's own name, path and URI, its
rename and its deletion, off the button before its name in the top bar (see
[editing](editing.md)) — each the label of the button that opened it, so
that the two cannot disagree — where the zoom and pixel menus name their sections in the
accent and are not headed: a percentage or a fit says what it is, and a bare
list of names or of applications does not say what pressing one would do with
them. The name is set as the help popup's column headings are, in bold on the
same band of `Theme::heading` over the same hairline, and `help::band` paints
both; `menu::titled` is the wrapper. A menu is as wide as its widest item,
which is not known until the items are laid out, so the band goes into a shape
set aside before the title and is painted last, to the width the menu came
out at.

The help popup, `src/ui/help.rs`, is the one popup hung off nothing: it is
anchored in the middle of the content area as the [chooser](chooser.md) is
anchored at the top of it, and opened by `App::press` on `Control::Help`
exactly as the chooser is, so `?`, `/` and the button at the foot of the
right strip all go through the same arm. What it lays out is the key table
itself, handed over as `Naming::help` — one `help::Section` per
`input::Section`, one `help::Row` per `keymap::Row`, its key column spelled
from the chords in force, and then the mouse's gestures — so that the popup,
`--help` and the manual page cannot list different keys; `cli.rs` derives
its headings from the same `Section::title`. The third column is
`Row::when`,
the condition on which a key does anything, kept apart from `help` because
`--help` has no column for it and a sentence that carried both would be
twice as long. A key whose action does
something else with a region up — `Space`, `Ctrl+C` — is two lines for the
same reason: its own, and a `Keys::Also` line under `When::RegionSelected`
describing the same name's chords, in the region's own section except for
`Ctrl+C`'s, which is a copy first and stays beside the image's. The table
dispatches once, since `perform` decides what the action does from the
selection, and describes twice; `Keymap::row_for` never answers with a line
that only describes, since what asks is a button that does the plain thing.
The arrows are different: with a region up they run other actions under
names of their own, in the region's context — see
[keys and gestures](keymap.md#contexts). `When` is one variant per
condition rather than the words themselves, so that the popup can say
whether it holds as well as what it is: `App::conditions` reads each off the same state the key's own
arm of `perform` reads, `Namer` carries the answers into the frame as
`Conditions`, and `help_sections` marks each row's `help::Condition` met or
not. The same reading is what makes a control dead: `Conditions::reasons`
is the tooltips' view of it and `App::refuses` the press's, so a button
drawn dead, its label and its press cannot disagree. A row whose condition does not hold is set in the dim ink throughout,
its condition in `Theme::caution` — what the row says is still true, and
what it needs is what is missing — so that the keys that would do something
right now are the ones that stand out. Nothing on the popup can be pressed,
which is why it reads the table rather than being handed rows to hand
presses back from.

egui has no layout that answers to its own width, so the table's is written
out: above `help::STACK_BELOW` a row is three columns, the key and the
condition at fixed widths and the description wrapping in what is left;
under it the three go one beneath the other with the whole width each, and
the column headings, which would then head nothing, are left out. The rows
are not striped: they are of mixed height, so a ledger's alternating wash
would read as uneven blocks rather than as ruling. The threshold is where
the description's column would otherwise be down to a few words a line —
and, not much narrower, to less than nothing, which egui refuses to lay
out. The headings
are laid out above the scroll area rather than in it, so they stay while the
rows go by under them, set in bold on a band of `Theme::heading` that runs
edge to edge of the popup inside its stroke, with the information panel's
hairline under them saying the same thing it says there. The headings share
the rows' width, which is why the scrollbar is always shown in its gutter,
as the information panel's is: a bar that came and went would move the rows'
right edge and not the headings'. The popup's own floor is the panels':
`PANEL_WIDTH` wide and
`INFO_MIN_HEIGHT` tall, inside the same padding, so `help::panel` is `None`
in exactly the content area `info::panel` is, `Room` carries a `help` beside
its `histogram` and `info`, and the button goes dead with theirs — the same
`NO_ROOM` on it, and the key refused in `App::press` as the toggles are.

The panels are opaque, and the image is drawn in the `Viewport` they leave
rather than behind them: zoom, fit, pan limits and the wheel's anchor are all
measured against that rectangle. It is derived per frame from the window and
whether the panels are showing, never stored, so `` ` `` re-fits a fitted
image without anything having to notice that it should.


## Opening in another application

The button under the copy button hands the file on screen to something else,
and the menu it opens is read out of the desktop's own database rather than
guessed at: `src/openers/linux.rs`. Every installed program ships a desktop entry
naming the MIME types it opens, `update-desktop-database` indexes those into a
`mimeinfo.cache` beside them, and the user's `mimeapps.list` says which is the
default and what associations they have added or removed by hand. Those three
files are the whole answer, and it is the same one a file manager's "Open
With" shows, because there is nowhere else it lives. Nothing is shelled out
to: `xdg-open` knows only the default and could not fill a menu, and `gio`
would be a runtime dependency on a package the user may not have.

Which file is which type is decided by the extension — `media.rs::MIME_TYPES`, one entry
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
path, `%u` and `%U` the `file:` URI that `uri::file` writes, and an
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


## The empty window and the file dialog

A program started with no path opens on nothing — and comes back to
nothing when the last file on the list is deleted — and `ui/empty.rs` is
what it shows: three buttons in the middle of the content area — the desktop's
file dialog for image files, the same dialog for a folder, and a paste of
the picture on the clipboard — each printing the key that does the same
thing from anywhere, and each a press of the same `Control` its key goes
through, so the two cannot drift. The state is not merely `current` being
`None`: that is also the moment before the first file arrives, and the
buttons up for the length of a decode would be a window changing its mind.
`App::is_empty` asks for nothing on screen *and* nothing in flight, and
`FrameInput::empty` carries the answer. The window comes back to the same
state when everything it was handed fails to open, with the message about
the failure under the buttons, where a picture would have had it under the
panels.

A window emptied by the last file leaving the list offers a fourth button
above the three, `Control::OpenLastFolder`: every image in the folder that
file came from. `App::last_folder` is the folder of the last picture to
arrive, made whole in `App::apply` so that it means the same after the
working directory has stopped mattering; `App::leave_picture` reads that
folder once, and keeps it in `App::offered_folder` only while it still
holds images, since the folder a list was culled from is often the one
that has just been emptied — reading it on every empty frame instead would
put a directory listing on the frame's path. The press is `App::open_named`
with the folder, exactly what choosing it in the dialog does. The dialog
itself starts in `last_folder`, passed to the portal as `current_folder`.

Nothing about the empty window is a mode. The keys stay on the one table;
the buttons about the picture in the strips — the copies and the region —
are drawn dead with `NOTHING_OPEN` for their reason, and the toggles that
outlast any picture stay live, since what they set is waiting for the next
one. The strip's own paste button is left out while the middle one is up,
one control being enough for one thing. `resumed` asks for a first frame
outright, since with nothing on the way there would otherwise be nothing
to ask for one; and `App::leave_picture`, which takes the last deleted
picture down, tells the renderer to `clear_image` so that the frame draws
the backdrop alone, as the first did.

The first picture to arrive in an empty window sizes it, as the first file
sizes the window at start-up: `App::size_to_next` is set while the window
shows nothing and spent by the arrival in `App::apply`, which runs the
picture's size through the same `window::initial_window_size` that
`resumed` used — the window's own account of the monitors standing in for
the event loop's — and `App::size_window_to` asks for that size. A window
opened at `--size` is not resized, that having been a choice rather than a
default. The fit follows on its own: the view is reset for a new picture,
and a fitted view is re-fitted against whatever viewport the next frame
has.

The ask is made two ways, because a Wayland window's size is the
compositor's and winit is careful about it. `request_inner_size` is the
toolkit's way: on Wayland it resizes the surface outright and is answered
at once, with no `Resized` event to follow, so the renderer is told the
new size on the spot. But winit refuses the request on any window whose
last configure carried a tiled state, and Hyprland sends the tiled edges
to every window it has, floating ones included, so on that desk the
request is a no-op. So the window's least and greatest size are also
pinned to the size wanted — `set_min_inner_size` and `set_max_inner_size`,
which reach the compositor as `xdg_toplevel` constraints it honors on its
next configure, resizing a floating window and re-centering it as a fresh
open would. That configure arrives as `Resized`, which lets the constraints
go again through `App::release_size`; a compositor that answers with
nothing — the window tiled — has them let go after `SIZING_GRACE` from
`about_to_wait`, since a window that could not be resized by hand
afterwards would be worse than one that stayed small.

Two buttons for the dialog because that is how every desktop's dialog is
built: it picks files or it picks a folder, never both in one, and a
program that offers both puts up two. `Ctrl+O` and `Ctrl+Shift+O` are the
same two presses from any window, bound one case each as the two `C`s of
the clipboard section are.

The dialog is the desktop's, asked for through `xdg-desktop-portal`'s
`FileChooser` — the one dialog a Wayland program can put up that looks like
the rest of the desk and carries the user's bookmarks. The ask is one
method, `OpenFile`, and the answer is a `Response` signal on a request
object, which arrives whenever the user has chosen. `src/portal/freedesktop.rs` makes
the call: the filter is one glob per extension the decoders read, in each
case, since a portal's globs match by their letters; the request's handle
is named ahead of the call with `handle_token` and subscribed to before it,
because the portal may answer before it has returned the handle, and a
signal nobody was waiting for is lost. The window is left unparented — a
parent takes an exported handle the toolkit does not hand out — and the
compositor places the dialog itself. The URIs that come back are `file:`
URIs, undone into the bytes of the path, so a name that is not UTF-8
survives.

The whole exchange blocks, on a thread of its own, and the answer comes
back through the event loop as `UserEvent::Picked`, the way a decode does.
`App::picking` is the one bit of state: set as the thread starts, cleared
as the answer lands, and what draws the two buttons dead — with
`DIALOG_UP` for their reason — and makes the keys refuse a second dialog
under the first. The thread is not joined: a dialog left up is up for as
long as the user leaves it, and the process leaving takes the dialog down
with it, the portal closing a request whose sender has gone.

The bus is spoken directly, in `src/dbus.rs`, for the reason the clipboard
and the monitors are: the whole need is one connection, three calls and one
signal, and the crates that do it bring an async runtime and forty crates
behind them. What is there is the wire format — a `Value` marshaled and
unmarshaled by signature, with the alignment and the array lengths the
specification asks — and a blocking `Connection` that authenticates with
`EXTERNAL`, says `Hello`, and reads messages one at a time, keeping any
signal that arrives while a reply is being waited for. A message is
refused before anything is allocated for it if it claims more than the
specification's maximum, and every read is bounded by the body's length,
since what is on the other end is a peer this program did not write.

What the dialog chose is opened as a command line naming it after the
rest would have: `App::open_named` runs the names through
`listing::expand` — a folder for the images in it, and a folder holding
none refused before anything moves — and `Files::append` puts the
newcomers at the end of the list, a path already on it not twice, and
asks for the first of them as a walk over the newcomers alone, so a file
that will not decode is stepped over as it is at start-up. The names join
`App::named` as well, those not already there, so a chosen directory is
watched and a rebuild of the list keeps the newcomers in the order they
were chosen; where nothing chosen is new, the first of it is gone to by
name, the way a row of the chooser is. Opening adds rather than replaces:
the session is the list, `]` and the chooser walk all of it, and a
picture opened is one the user can step back to. The picture on screen
stays until the first newcomer arrives, exactly as for a step, and a file
coming back through the dialog into an empty window is restored as a step
back to it would be: `App::apply` looks the arriving file up in `kept` for
any fresh read, not only one arriving beside a picture.

Whether nothing showing means leaving is `App::from_command_line`. A
command line whose every file fails to decode is answered by leaving with a
failing status, as it always was; a choice made in the window that fails is
answered in the window, which stays up for the next choice, and a program
opened on nothing and closed on nothing has not failed.
