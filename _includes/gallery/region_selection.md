Use the crop icon on the left or `x` to open the region panel and start drawing a region. Once a region is selected, the following will work:

* The region panel buttons will work (fit, copy, clear, and aspect swap).
* Auto-fitting (`Space`) will fit the region before fitting the whole image
* Image copy (`Ctrl+C` / `⌘C`) will copy the region instead of the whole image
* Region-specific keys will become active

You can choose an aspect ratio in the drop down: Free (default), Original, 1:1, 5:4, 4:3, 7:5, 3:2, 16:9. Or type it in the box as two numbers and a separator (`: / x X ×`), or a single number.

X, Y, W, and H fields can be used to exactly specify where and how large to crop.

Units for the fields and the labeled display are shown in the [current pixel coordinate unit](#pixel-copy) (pixels, [print measurement](#print-size), or georeferenced map units) registered to the nearest pixel.

When a region is showing, one of its handles (starting with the center) is always active. Clicking on a handle activates it. The key effect depends on which handle is active.

| Handle | Linux | macOS | Description |
|--|--|--|--|
| Center | `↑ ↓ ← →` | `↑ ↓ ← →` | Shift the region by one pixel in the arrow direction |
| Center | `Ctrl+↑ ↓ ← →` | `⌘↑ ↓ ← →` | Grow the selection by one pixel in the arrow direction |
| Center | `Ctrl+Shift+↑ ↓ ← →` | `⇧⌘↑ ↓ ← →` | Shrink the selection by one pixel in the arrow direction |
| Corner | `↑ ↓ ← →` | `↑ ↓ ← →` | Grow or shrink selection by one pixel on that corner |
| Side | `↑ ↓ ← →` (perpendicular to side) | `↑ ↓ ← →` (perpendicular to side) | Grow or shrink selection by one pixel on that side |
| Side | `↑ ↓ ← →` (parallel to side) | `↑ ↓ ← →` (parallel to side) | Shift selection by one pixel |

The keys make it quite simple to select the exact area you want.
