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

Zoom runs from 2% to 6400%. Zooming leaves fit mode; panning does not, so `f`
and then Down scrolls through a tall image at fit-width.

`n` and `p` keep the pan and zoom when the next file is the same size as the
one on screen, so a directory of frames or exposures stays comparable under
the same pixels. A file of a different size is a different picture, and is
fitted. Files that cannot be decoded are stepped over.

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
| `m` | Show or hide the minimap |
| `` ` `` | Show or hide the panels around the image |

The panels are opaque and the image is fitted inside them, so hiding them
gives a fitted image more room and it re-fits immediately.

## The mouse

| Action | What it does |
| --- | --- |
| Drag | Pan, with the image following the pointer |
| Wheel | Zoom about the pointer |
| Trackpad scroll | The same, by fractions of a notch |
| Click a panel button | Show or hide the histogram or the minimap |
| Click the zoom percentage | Open the zoom menu |

The percentage at the end of the bottom bar is a button. Pressing it opens a
menu in the lower right corner: 10% through 1600%, and the three fits — the
whole image, its width, its height — as arrows pointing the way each one
fills the window. Whichever the view is in is lit. Choosing sets the zoom;
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
`--histogram` and `--minimap`.

## Keys that are deliberately ignored

Anything held with `Ctrl`, `Alt` or a `Super`/`Command` key does nothing here,
and neither does `Ctrl` with the wheel. Those combinations belong to the
window manager, and a chord such as `Super+0` would otherwise move the view
behind its back.
