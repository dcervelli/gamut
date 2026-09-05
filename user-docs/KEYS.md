# Keys and mouse

Every control `gamut` has. Letter keys work in either case except `a`,
`s` and `c`, where the two cases do different things.

## Zoom and position

| Key | What it does |
| --- | --- |
| `1`, `0` | Actual size, one image pixel per screen pixel |
| `2`, `3`, `4`, `5` | 200%, 400%, 800%, 1600% |
| `Shift`+`2`, `3`, `4` | 50%, 25%, 10% |
| `+`, `=` | Zoom in one step, a factor of 1.25 |
| `-`, `_` | Zoom out one step |
| `Space` | Cycle how the image is fitted: whole → width → height |
| `p` | Cycle the filter used above 100%: nearest → bicubic |
| Arrows | Pan by 64 pixels |
| `Shift`+Arrows | Pan by one pixel |
| `Ctrl`+Arrows | Pan to the far side of the image |

Zoom runs from 2% to 6400%. The zooms below 100% are the ones above it with
`Shift` held, so each zoom is under the number it hangs off. The number row is
the one part of the keyboard read by where a key is rather than by what it
types, so `Shift`+`2` is 50% whatever character your layout puts there.

Zooming leaves fit mode; panning does not, so `Space` and then Down scrolls
through a tall image at fit-width.

`Shift` with an arrow places the view to the pixel, which is what lining two
images up on the same detail takes; `Ctrl` with one runs to that side of the
image in a single press. Neither does anything when the whole image is already
on screen, since there is then nowhere to pan to.

## Moving through the files

| Key | What it does |
| --- | --- |
| `]`, `Page Down` | Next file |
| `[`, `Page Up` | Previous file |

A file you have already looked at comes back exactly as you left it: the same
pan and zoom, the same window and exposure, the same tone curve and false
color. Flipping between two images with `[` and `]` therefore compares them
rather than resetting them.

A file being opened for the first time keeps the pan and zoom when it is the
same size as the one on screen, so a directory of frames or exposures stays
comparable under the same pixels. One of a different size is a different
image, and is fitted. Files that cannot be decoded are stepped over. The list is whatever
was named at startup, in that order; naming a directory puts the images in it
on the list, and keeps it up to date as images are added to that directory or
taken out of it.

## Copying and pasting

| Key | What it does |
| --- | --- |
| `c` | Copy the name of the file on screen, without its path |
| `Shift+C` | Copy the absolute path of the file on screen |
| `Ctrl+Shift+C` | Copy the file on screen as a URI |
| `Ctrl+C` | Copy the image itself, as you are seeing it |
| `Ctrl+I` | Copy everything the file information says about the file |
| `Ctrl+.` | Copy the value of the pixel under the pointer |
| `Ctrl+Shift+.` | Copy the coordinate of the pixel under the pointer |
| `Ctrl+V` | Paste an image, saved among your pictures and shown |

Every copy says so: a short message appears at the foot of the window naming
what was taken, and goes on its own after a couple of seconds. Copying changes
nothing you can see in the image, so without it there is no way to tell a
copy that worked from a key that was not read. Click the cross on the message
to take it off sooner, or press `Esc` — which closes whatever is up, a popup
first and then a message, and quits only when there is nothing left to close.
`q` quits whether or not a message is showing. A message in the theme's yellow
means what you asked for could not be done and nothing is wrong — the pointer
was not over a pixel, say; one in its red means the copy failed, and the
terminal has the reason in full.

`c` copies the file's own name and nothing else — `sunset.tif`, not the
directory it sits in — which is what you want when you are naming the file to
someone rather than pointing a program at it.

`Shift+C` copies the path in full, from the root down, whichever way the file
was named when the program was started — a relative name is of no use in
another window, which is where a copied path is going.

`Ctrl+Shift+C` copies the same file as a URI instead: `file://` and the path,
with spaces and punctuation escaped. This is what a file manager, a browser or
another program's open dialog asks for when it wants the file itself rather
than words about it, so pasting into one of those opens the image rather
than typing its name. Pasting into a text field still yields the URI.

`Ctrl+C` copies the image rather than a name for it, ready to paste into an
editor, a document or a chat window. What travels is what you are looking at:
the window, the exposure, the tone curve and any false color are all applied,
so a raw scan you have brought up out of the shadows arrives brought up. It is
the image at its own size, not a photograph of the window — the zoom, the pan
and the panels are how you are looking at it and none of them are copied.

A single-channel image stays single-channel, so a grayscale scan does not
arrive as three copies of itself, and false color is the one thing that
widens it. Transparency comes along only where the file had some; an opaque
image arrives opaque rather than carrying an empty channel. Everything
arrives 8-bit, which is the depth the screen was showing it at.

`Ctrl+I` copies everything the file information panel says, one line per
field: the section it stands under, its name, and what it says, separated by
commas so that it opens as a table in a spreadsheet and reads as plain lines
anywhere else. It works whether or not the panel is showing — what it says is
a fact about the file, and asking for it should not mean first arranging to
look at it. Single fields and single sections can be copied from the panel
itself; see below.

`Ctrl+.` copies the value the bottom left corner is reading out, exactly as it
is written there — so switching the readout to hex with `>` and pressing
`Ctrl+.` puts `E78040` on the clipboard, and switching it back puts
`231 128 64` there instead. `Ctrl+Shift+.` copies the pixel's coordinate
rather than its value, as `x,y` with nothing around it: `1919,1079`, ready to
paste into a command line or a spreadsheet. Both work on the pixel the pointer
is over at the moment you press them, and say so on the terminal when the
pointer is not over one.

The image is prepared in the background, so the window keeps answering while
a large one is being got ready — a photograph of some tens of megapixels takes
a fraction of a second, and only then is there anything to paste. Copying
something else in the meantime wins: whichever copy you asked for last is the
one you get, not whichever happened to finish last.

Any of the copies outlives the window: closing `gamut` leaves it on
the clipboard, and it stays there until something else copies over it. Quitting
straight after copying is safe — an image still being prepared is finished
before the window goes.

`Ctrl+V` goes the other way: it takes the image on the clipboard, writes it
into your pictures directory, and shows it. The file is a real one and it
stays — a screenshot, or an image copied out of a browser or an editor, has
no file behind it, and one that vanished when you closed the window would be
no use to come back to. It is named for the moment you pasted it, in the same
form a screenshot is named in: `pasted_2026-09-04_11-40-32.png`. Where it
goes is wherever your desktop keeps pictures — `~/Pictures` unless you have
told it otherwise — and the directory is made if it is not there yet.

There is a button for it too, in the left strip under the copy button. It
is there only while the clipboard is holding an image `gamut` can show, so
what it says is not only that pasting is possible but that there is something
to paste — it appears when you copy an image in another window and goes when
something else is copied over it. `gamut` looks at the clipboard four times a
second to keep it up to date, and stops looking while the interface is hidden
with `` ` ``.

The pasted file joins the list beside the one you were looking at, so `[`
goes back to where you were and `]` carries on. It stays on the list for as
long as the window is open, even when what you opened was a directory that
knows nothing about it.

What is pasted is whatever the image was copied as, saved as it stands:
nothing is re-encoded, so a JPEG arrives a JPEG. `Ctrl+V` does nothing if the
clipboard holds words rather than an image, or holds it in a format `gamut`
cannot read — it says so on the terminal and leaves the window as it was.
Copying a *file* in a file manager copies its name and not its contents, and
that is not a paste; open it as an argument instead.

## The display

| Key | What it does |
| --- | --- |
| `d` | Exposure down half a stop |
| `f` | Exposure up half a stop |
| `a` | Slide the window down |
| `s` | Slide the window up |
| `A` | Narrow the window, raising contrast |
| `S` | Widen the window, lowering contrast |
| `e` | Cycle the automatic window: off (0–1) → min/max → 99.8% |
| `t` | Cycle tone mapping: none → Reinhard → neutral |
| `o` | Turn the room above white off and on, where the monitor is in HDR mode |
| `r` | Cycle false color: gray → viridis → magma → turbo |
| `z` | Reset every display setting |

The window is the one place the case of a key matters: `a` and `s` move it,
`A` and `S` change how wide it is.

Sliding or resizing the window by hand takes it out of whichever automatic
mode it was in; `e` cycles back into them. Exposure stops at ±16 stops.

Tone mapping is a curve added to bring values brighter than white down into a
surface that cannot show them. `none` is not a third curve but the absence of
one: on an ordinary (SDR) surface the highlights are clipped at white, and on
an HDR surface they are shown at the brightness they were graded to. An image
starts with no curve on an HDR surface; on an SDR one it starts on neutral
when there are highlights above white to roll off, and with none otherwise.
Switching the room with `o`, or the window landing on a different kind of
monitor, chooses the curve again for what it lands on; `t` changes it after
that.

The bottom bar names the curve while one is on, and says `clip` when there is
none, the surface is SDR and highlights are being thrown away — so a
photograph pushed a stop up says so rather than going flat in silence. The
`HDR` button at the end of the bar is the switch for that room, lit while the
image is going out with it. On Wayland the compositor says which monitors
are in HDR mode, and gamut follows: the window gets an HDR surface on a
monitor in HDR mode and an SDR one otherwise, and never asks the compositor
to switch a monitor over — a request some compositors answer by blanking
every display. On a monitor in HDR mode the button turns the room off and on
without touching the surface. On one in SDR mode, or where no HDR color
space is offered, it is drawn dead and `o` does nothing but say why in the
terminal. `--output hdr` asks for the HDR surface regardless, for anyone who
wants the compositor's own switch, and `--output sdr` stays on the SDR one
whatever the monitor is. Where nothing says what the monitor is, `o` moves
the surface itself.

False color applies to single-channel images only, and `r` does nothing on a
color one. While a colormap is active, tone mapping is suspended — a curve on
top of a colormap would distort the values you are reading off it.

## The interface

| Key | What it does |
| --- | --- |
| `h` | Show or hide the histogram |
| `l` | Count the histogram's bars up its axis, or the logarithm of them |
| `i` | Show or hide the file information |
| `m` | Show or hide the minimap |
| `g` | Show or hide the grid over the image |
| `>` | Cycle how the pixel under the pointer is read out: hex → decimal → mapped |
| `` ` `` | Show or hide the panels around the image |
| `~` | The same, and closes the histogram, information and minimap |
| `q`, `Esc` | Quit. `Esc` closes an open popup or message first |

The panels are opaque and the image is fitted inside them, so hiding them
gives a fitted image more room and it re-fits immediately.

The histogram plots each bar as its share of the fullest one, which is the
plot a photograph wants. It is the wrong plot for measurement data, where one
value often covers most of the image — a masked sea, the black surround of a
scan — and that one bar flattens everything the rest of the range is doing
into the axis. `l`, or the curve button beside the plot, counts the
logarithm instead: the tall bar stays at the top and the short ones rise to
where they can be read beside it. Heights can no longer be compared with each
other once it is on, which is the point of it being a switch. It applies to
whichever image is on screen and stays as you set it, and setting it while the
histogram is closed leaves it that way for when you open it.

The histogram, the file information and the minimap float over the image
rather than sitting in the bars, so `` ` `` leaves them where they are.
`Shift` with it closes all three as well, for the image on its own; they
stay closed when the bars come back.

The file information sits down the right of the image, under headings, so
that a long column can be read by looking for a thing rather than from the
top. It opens with the file itself — what it is called, where it is, what it
turned out to be, how large it is and when it was last written — and then the
image in it: how many pixels across and down, what each pixel holds, the
color space those numbers are meant in, and whether it carries transparency.

After those comes what the file's own metadata says, for a file that carries
any: the camera and lens, when the photograph was taken, the exposure it was
made at and the focal length; then where the camera stood, in degrees a map
will take, with whatever else it recorded about the place. A georeferenced
raster — a scanned map, an elevation model — gets a section of its own: the
coordinate system it names, the size of a pixel on the ground, where its
corner sits, the ground it covers, and the value that stands for nothing
measured. Anything somebody wrote in words — a description, a comment, who
made the file and what may be done with it — is drawn out into a section of
its own too, and everything left over is listed after all of them, field by
field, as the file gives it.

Clicking copies. A click on a field copies what it says; a click on a heading
copies that whole section, one line per field as a name and a value; and the
button at the top of the panel copies the lot, with the section named on each
line as well — the same thing `Ctrl+I` does. A mark appears over whatever the
pointer is on to show what a click would take. It goes away while the column
is being scrolled, since the pointer is then resting rather than pointing.

Coordinate systems are quoted as the file gives them, by name and by EPSG
code. Turning a code into a projection and a datum needs a register this does
not carry, so what you get is what was written.

The file's own date is in UTC; the date the photograph was taken is whatever
the camera recorded, with the offset from UTC it was set to where it recorded
one. Metadata is read from JPEG, TIFF, PNG, WebP and HEIF files, and from a TIFF
however large it is and wherever in the file it keeps it. Where it has more to say than
fits, the wheel scrolls it — point at the panel rather than at the image, and
the wheel moves the words instead of the zoom. Dragging the panel scrolls it
as well, the drag holding the scrollbar's handle rather than the words: drag
down to move down the column, and a short drag carries a long column a long
way — as far as putting the handle there would. The pointer belongs to the
panel while it is over it, so neither gesture reaches the image behind.

The grid divides the image into squares of a round number of image pixels —
1, 2, 5, 10, 20, 50 and so on — chosen so that the lines land about fifty
screen pixels apart at whatever zoom the view is at. The button at the end of
the top bar says which spacing is in force, so a distance on screen can be
counted off in the image's own pixels.

## The mouse

| Action | What it does |
| --- | --- |
| Drag | Pan, with the image following the pointer |
| Wheel | Zoom about the pointer |
| Trackpad scroll | The same, by fractions of a notch |
| Click a panel button | Show or hide the histogram, the file information, or the minimap |
| Click the grid button | Show or hide the grid |
| Click the copy button | Open the menu of copies: the file, or the image |
| Click the paste button | Paste the image on the clipboard, as `Ctrl+V` does |
| Wheel over the file information | Scroll it |
| Drag the file information | Scroll it, as if dragging the scrollbar's handle |
| Click the zoom percentage | Open the zoom menu: scale, fit and the magnification filter |
| Click the dot in the bottom left | Choose how a pixel's value is read out |

The button under the minimap toggle, in the left strip, opens a menu of the
copies beside it: the file's **Name**, its **Path**, its **URI**, the
**Info** the file information panel holds about it, and the **Image** itself.
Each cell does exactly what its key does, and resting on one names the key as
well, so the menu is also where those keys are learned. The two copies
of the pixel under the pointer are not on it: the pointer is over the menu
while the menu is open, so there would be no pixel under it to copy. Pressing
anywhere outside the menu, or `Esc`, closes it without copying anything.

The percentage in the top bar, just inside the grid button, is itself a
button. Pressing it opens a menu hanging under it, under three headings.
**Zoom** is 10% through 1600%. **Fit** is the three fits — the whole image,
its width, its height — as arrows pointing the way each one fills the window.
**Up-scaling** is the filter the image is magnified with, `Nearest` or
`Bicubic`, the same choice `p` cycles. Whichever of each the view is in is
lit, and choosing acts at once; pressing anywhere outside the menu, or `Esc`,
closes it without changing anything. A window too small to hold the menu does
not open one.

Pointing at the image reads that pixel out in the bottom left corner: its
coordinates, a swatch of the color it comes out on screen, and its value. The
swatch is what the display settings actually make of the pixel, false color
included; the coordinates are padded to the size of the image, so nothing
after them moves as the pointer crosses a power of ten. In a window too narrow
for all of it, the coordinates stay.

One pixel answers more than one question, so the value is written whichever of
three ways you ask for. **Decimal** is the numbers the file holds, in its own
units — codes for an 8-bit image, counts for a 16-bit one, the value itself
for floating point — the numbers whatever wrote the file put there. **Hex** is
those same numbers as a color is usually written down: run together, in upper
case, with no `#` and no `0x`, two digits to an 8-bit sample and four to a
16-bit one, so an ordinary photograph reads `E78040` and pastes straight into
anything that takes a color. A floating-point file has no such code, and
what you get there is the bits it actually stores. **Mapped** is what the
window, the exposure and the tone curve have made of the numbers, where 0 and
1 are the ends of the window the bar names on the right.

`>` steps through the three, and the dot at the head of the readout opens a
menu of them; whichever is in force is lit. It applies to whichever image is
on screen and stays as you set it.

The pointer keeps its grab until the button comes up, so a drag that leaves
the window goes on working. The cursor becomes a closed hand only when there
is somewhere to drag to.

The panels come between the pointer and the image. Anything on screen over
the image — the histogram, the file information, the minimap, an open menu —
takes what the pointer does while it is over it, so a click there never starts
a drag of the image behind, the wheel there never zooms, and the readout in
the bottom bar goes quiet rather than naming a pixel the panel is covering.
Hide a panel to get that ground back, or point at the image somewhere else.

## Starting somewhere other than the default

Most of what these keys reach can be set before the first file opens, which is
what scripting wants and what comparing two files on equal terms needs:
`--exposure`, `--window`, `--tone-map`, `--colormap`, `--upscale`,
`--histogram` and `--info`. The minimap starts on; `--no-minimap` starts
without it.

## Keys that are deliberately ignored

Apart from the copying chords and `Ctrl` with an arrow, anything held with
`Ctrl`, `Alt` or a `Super`/`Command` key does nothing here, and neither does
`Ctrl` with the wheel. Those combinations belong to the window manager, and a
chord such as `Super+0` would otherwise move the view behind its back.
