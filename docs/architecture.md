# Architecture

Rendering is three separable layers:

1. **Image layer** → a linear working-space target. Swizzle, primaries
   matrix, display window, false color.
2. **UI layer** → its own sRGB target. One instanced draw for every rectangle
   plus one text pass.
3. **Compositor** → the surface. Tone map, lay the UI over, encode for
   whatever the surface turned out to be.

Keeping the interface off the image's target is what lets UI code stay in
plain sRGB and logical pixels while the image beside it is extended-range
linear — and it is what makes a third-party text renderer usable at all, since
glyphon has no idea what an HDR surface is.

`UiFrame` is a display list, not a widget toolkit:

```rust
frame.rounded_rect(Rect::new(x, y, w, h), 6.0, theme.panel_background);
frame.text([x, y], 13.0, theme.text_primary, "hello");
let width = renderer.measure_text("hello", 13.0)[0];   // for layout
```

Adding panels, sliders or inspectors means emitting more primitives; the
renderer does not change, and it all batches into the same two draws.


How those layers are dressed — the panels, the pointer, the information
column, the menus — is [the interface](interface.md).

## Where things live

| File | Role |
| --- | --- |
| `src/main.rs` | Event-loop setup |
| `src/cli.rs` | Argument parsing and `--help`, whose key sections come from the keymap |
| `src/app/` | Window lifecycle and the event loop's state: `files.rs` is the file list and the read in flight, `input.rs` the keymap and pointer, `window.rs` titles and opening size |
| `src/ui/` | Building each frame's interface: `chrome.rs` the panels, one file per widget, `pixel.rs` the pointer's readout, `status.rs` the words in the bars |
| `src/view.rs` | Zoom / pan / fit geometry — pure maths |
| `src/listing.rs` | What a path on the command line stands for: a directory is the images inside it, read again while the program runs |
| `src/watch.rs` | Noticing that the file on screen has been rewritten, or that a directory named on the command line holds something else now |
| `src/clipboard.rs` | The clipboard and `file:` URIs, held by a process of its own so a copy outlives the window; and reading a pasted picture off it |
| `src/pasted.rs` | Where a pasted picture is written and what it is called, by the desktop's own conventions |
| `src/clock.rs` | A moment as a date and time, in UTC or in the zone the system is set to |
| `src/theme/` | `palette.rs` reads the desktop's palette; `mod.rs` derives the colors drawn from it |
| `src/image/` | The data model: `Samples`, `color/` (transfer functions, primaries, ICC and CICP), stats, display state |
| `src/image/decode/` | The decoder trait and its registry, one file per format |
| `src/image/encode.rs` | The display pipeline run over every pixel, out to an 8-bit sRGB PNG |
| `src/render/` | Upload planning, the three layers, output selection; `shader_codes.rs` is every integer the shaders switch on |
| `src/render/ui_layer/popup.rs` | Where a popup menu's panel, headings and cells go, and what a press lands on |
| `src/render/reduce.rs` | The coarse chain a minifying draw reads from |

