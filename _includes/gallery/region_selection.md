Use the crop icon on the left or `x` to start drawing a region. Once a region is selected, the following will work:

* Auto-fitting (`Space`) will fit the region before fitting the whole image
* Image copy (`Ctrl+C` / `⌘C`) will copy the region instead of the whole image
* Region-specific keys will become active

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
