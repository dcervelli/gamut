# Keys and mouse

Every control `image-view` has. Letter keys work in either case except `e`
and `c`, where the two cases do different things.

## The view

| Key | What it does |
| --- | --- |
| `q`, `Esc` | Quit. `Esc` closes an open popup first |
| `+`, `=` | Zoom in one step, a factor of 1.25 |
| `-`, `_` | Zoom out one step |
| `0` | Actual size, one image pixel per screen pixel |
| `f` | Cycle how the image is fitted: whole → width → height |
| `u` | Cycle the filter used above 100%: nearest → bicubic |
| Arrows | Pan by 64 pixels |
| `n`, `Page Down` | Next file |
| `p`, `Page Up` | Previous file |
| `Shift+C` | Copy the absolute path of the file on screen |
| `Ctrl+Shift+C` | Copy the file on screen as a URI |
| `Ctrl+C` | Copy the picture itself, as you are seeing it |
| `Ctrl+M` | Copy everything the file information says about the file |

Zoom runs from 2% to 6400%. Zooming leaves fit mode; panning does not, so `f`
and then Down scrolls through a tall image at fit-width.

`n` and `p` keep the pan and zoom when the next file is the same size as the
one on screen, so a directory of frames or exposures stays comparable under
the same pixels. A file of a different size is a different picture, and is
fitted. Files that cannot be decoded are stepped over. The list is whatever
was named at startup, in that order; naming a directory puts the images in it
on the list.

`Shift+C` copies the path in full, from the root down, whichever way the file
was named when the program was started — a relative name is of no use in
another window, which is where a copied path is going.

`Ctrl+Shift+C` copies the same file as a URI instead: `file://` and the path,
with spaces and punctuation escaped. This is what a file manager, a browser or
another program's open dialog asks for when it wants the file itself rather
than words about it, so pasting into one of those opens the picture rather
than typing its name. Pasting into a text field still yields the URI.

`Ctrl+C` copies the picture rather than a name for it, ready to paste into an
editor, a document or a chat window. What travels is what you are looking at:
the window, the exposure, the tone curve and any false colour are all applied,
so a raw scan you have brought up out of the shadows arrives brought up. It is
the image at its own size, not a photograph of the window — the zoom, the pan
and the panels are how you are looking at it and none of them are copied.

A single-channel image stays single-channel, so a greyscale scan does not
arrive as three copies of itself, and false colour is the one thing that
widens it. Transparency comes along only where the file had some; an opaque
picture arrives opaque rather than carrying an empty channel. Everything
arrives 8-bit, which is the depth the screen was showing it at.

`Ctrl+M` copies everything the file information panel says, one line per
field: the section it stands under, its name, and what it says, separated by
commas so that it opens as a table in a spreadsheet and reads as plain lines
anywhere else. It works whether or not the panel is showing — what it says is
a fact about the file, and asking for it should not mean first arranging to
look at it. Single fields and single sections can be copied from the panel
itself; see below.

The picture is prepared in the background, so the window keeps answering while
a large one is being got ready — a photograph of some tens of megapixels takes
a fraction of a second, and only then is there anything to paste. Copying
something else in the meantime wins: whichever copy you asked for last is the
one you get, not whichever happened to finish last.

Any of the three copies outlives the window: closing `image-view` leaves it on
the clipboard, and it stays there until something else copies over it. Quitting
straight after copying is safe — a picture still being prepared is finished
before the window goes.

## The display

| Key | What it does |
| --- | --- |
| `e` | Exposure down half a stop |
| `E` | Exposure up half a stop |
| `a` | Cycle the automatic window: off (0–1) → min/max → 99.8% |
| `[` | Slide the window down |
| `]` | Slide the window up |
| `,` | Narrow the window, raising contrast |
| `.` | Widen the window, lowering contrast |
| `t` | Cycle tone mapping: clip → Reinhard → neutral |
| `c` | Cycle false colour: grey → viridis → magma → turbo. Lower case only |
| `r` | Reset every display setting |

Sliding or resizing the window by hand takes it out of whichever automatic
mode it was in; `a` cycles back into them. Exposure stops at ±16 stops.

False colour applies to single-channel images only, and `c` does nothing on a
colour one. While a colormap is active, tone mapping is suspended — a curve on
top of a colormap would distort the values you are reading off it.

## The interface

| Key | What it does |
| --- | --- |
| `h` | Show or hide the histogram |
| `i` | Show or hide the file information |
| `m` | Show or hide the minimap |
| `g` | Show or hide the grid over the image |
| `` ` `` | Show or hide the panels around the image |
| `~` | The same, and closes the histogram, information and minimap |

The panels are opaque and the image is fitted inside them, so hiding them
gives a fitted image more room and it re-fits immediately.

The histogram, the file information and the minimap float over the image
rather than sitting in the bars, so `` ` `` leaves them where they are.
`Shift` with it closes all three as well, for the picture on its own; they
stay closed when the bars come back.

The file information sits down the right of the image, under headings, so
that a long column can be read by looking for a thing rather than from the
top. It opens with the file itself — what it is called, where it is, what it
turned out to be, how large it is and when it was last written — and then the
picture in it: how many pixels across and down, what each pixel holds, the
colour space those numbers are meant in, and whether it carries transparency.

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
line as well — the same thing `Ctrl+M` does. A mark appears over whatever the
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
panel while it is over it, so neither gesture reaches the picture behind.

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
| Wheel over the file information | Scroll it |
| Drag the file information | Scroll it, as if dragging the scrollbar's handle |
| Click the zoom percentage | Open the zoom menu |

The percentage in the top bar, just inside the grid button, is itself a
button. Pressing it opens a menu hanging under it: 10% through 1600%, and the
three fits — the whole image, its width, its height — as arrows pointing the
way each one fills the window. Whichever the view is in is lit. Choosing sets the zoom;
pressing anywhere outside the menu, or `Esc`, closes it without changing
anything. A window too small to hold the menu does not open one.

Pointing at the image reads that pixel out in the bottom left corner: its
coordinates, the numbers the file holds there, an arrow, and what the display
settings map those numbers to. The numbers are in the file's own units — codes
for an 8-bit image, counts for a 16-bit one, the value itself for floating
point — so they are the numbers whatever wrote the file put there. The mapped
values are what the window, the exposure and the tone curve have made of them,
where 0 and 1 are the ends of the window the bar names on the right. The
swatch at the front is the colour the pixel comes out on screen, false colour
included. In a window too narrow for all of it, the coordinates stay.

The pointer keeps its grab until the button comes up, so a drag that leaves
the window goes on working. The cursor becomes a closed hand only when there
is somewhere to drag to.

## Starting somewhere other than the default

Most of what these keys reach can be set before the first file opens, which is
what scripting wants and what comparing two files on equal terms needs:
`--exposure`, `--window`, `--tone-map`, `--colormap`, `--upscale`,
`--histogram` and `--info`. The minimap starts on; `--no-minimap` starts
without it.

## Keys that are deliberately ignored

Apart from the two copying chords above, anything held with `Ctrl`, `Alt` or a
`Super`/`Command` key does nothing here, and neither does `Ctrl` with the
wheel. Those combinations belong to the window manager, and a chord such as
`Super+0` would otherwise move the view behind its back.
