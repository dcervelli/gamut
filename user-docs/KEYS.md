# Keys and mouse

Every control `image-view` has. Letter keys work in either case except `e`,
where the two cases move exposure in opposite directions.

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
| `Ctrl+Shift+C` | Copy the path of the file on screen |

Zoom runs from 2% to 6400%. Zooming leaves fit mode; panning does not, so `f`
and then Down scrolls through a tall image at fit-width.

`n` and `p` keep the pan and zoom when the next file is the same size as the
one on screen, so a directory of frames or exposures stays comparable under
the same pixels. A file of a different size is a different picture, and is
fitted. Files that cannot be decoded are stepped over. The list is whatever
was named at startup, in that order; naming a directory puts the images in it
on the list.

`Ctrl+Shift+C` copies the path as the program was given it, so a file named
relative to where you started stays relative. The copy outlives the window:
closing `image-view` leaves it on the clipboard, and it stays there until
something else copies over it.

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
| `c` | Cycle false colour: grey → viridis → magma → turbo |
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

The panels are opaque and the image is fitted inside them, so hiding them
gives a fitted image more room and it re-fits immediately.

The file information sits down the right of the image. It opens with the file
itself — what it is called, where it is, how large the image is, and when the
file was last written and to what size — and then, for a file that carries
any, what its metadata says: the camera and lens, when the photograph was
taken, the exposure it was made at, the focal length, and where the camera
was. A georeferenced raster — a scanned map, an elevation model — gets its own
section instead: the coordinate system it names, the size of a pixel on the
ground, where its corner sits, the ground it covers, and the value that stands
for nothing measured. Everything else the metadata holds is listed under that,
field by field, as the file gives it.

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

Apart from `Ctrl+Shift+C`, anything held with `Ctrl`, `Alt` or a
`Super`/`Command` key does nothing here, and neither does `Ctrl` with the
wheel. Those combinations belong to the window manager, and a chord such as
`Super+0` would otherwise move the view behind its back.
