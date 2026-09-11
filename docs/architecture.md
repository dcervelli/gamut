# Architecture

Rendering is three separable layers:

1. **Image layer** → a linear working-space target. Swizzle, primaries
   matrix, display window, false color.
2. **UI layer** → its own sRGB target. The interface is laid out by
   [egui](https://github.com/emilk/egui), tessellated on the CPU, and drawn by
   `egui-wgpu` into that target and nothing else.
3. **Compositor** → the surface. Tone map, lay the UI over, encode for
   whatever the surface turned out to be.

Keeping the interface off the image's target is what lets UI code stay in
plain sRGB and logical pixels while the image beside it is extended-range
linear — and it is what makes a third-party toolkit usable at all, since egui
has no idea what an HDR surface is. Given the target's sRGB format its
renderer converts its colors to linear in its own shader and lets the
attachment encode on write, and its blend state is premultiplied source-over,
which is exactly what `composite.wgsl` reads the target as; pointed at an
HDR surface directly, it would put sRGB-authored colors into a linear or PQ
signal with no conversion.

The interface is one function of the application's state:

```rust
let commands = ui::show(ui, &input, &panels, current, &view, &theme, &namer);
for command in commands {
    app.act(command);          // Press(Control), Drag, Wheel, OverImage
}
```

`ui::show` lays the whole window out — the four panels, the picture's own
response between them, and the areas floating over it — and never reaches
into the application: what was pressed comes back as `Command`s, done after
the frame is off through the same dispatch a key goes through. That is what
keeps `ui/` below `app/`, and what lets `ui/driven.rs` press the interface
with no application behind it. The words the interface needs — a tooltip,
the key printed beside a menu item — come down through `Naming`, which the
application implements from its key table.

The redraw stays on demand. egui says with each pass how soon it wants
painting again — at once for an animation of its own, at some moment for a
tooltip's delay, never until something happens — and `app/gui.rs` folds that
into the same deadline the file watch, the message's clock and the frame
clock of an animated picture sleep until.


How those layers are dressed — the panels, the pointer, the information
column, the menus — is [the interface](interface.md).

## Where things live

| File | Role |
| --- | --- |
| `src/main.rs` | Event-loop setup |
| `src/cli.rs` | Argument parsing and `--help`, whose key sections come from the keymap |
| `src/app/` | Window lifecycle and the event loop's state: `files.rs` is the file list and the read in flight, `input.rs` the keymap and what the interface asked for, `gui.rs` egui's context and its winit adapter, `window.rs` titles and opening size, `playback.rs` the clock an animation plays by |
| `src/player.rs` | The thread that decodes an animation's frames ahead of the clock, and the cache it keeps them in under a budget |
| `src/ui/` | Laying each frame's interface out with egui: `chrome.rs` the panels, one file per widget, `pixel.rs` the pointer's readout, `status.rs` the words in the bars, `style.rs` the theme as egui's style, `fonts.rs` the desktop's faces |
| `src/view.rs` | Zoom / pan / fit geometry — pure maths |
| `src/listing.rs` | What a path on the command line stands for: a directory is the images inside it, read again while the program runs |
| `src/watch.rs` | Noticing that the file on screen has been rewritten, or that a directory named on the command line holds something else now |
| `src/clipboard.rs` | The clipboard and `file:` URIs, held by a process of its own so a copy outlives the window; and reading a pasted picture off it |
| `src/pasted.rs` | Where a pasted picture is written and what it is called, by the desktop's own conventions |
| `src/clock.rs` | A moment as a date and time, in UTC or in the zone the system is set to |
| `src/theme/` | `palette.rs` reads the desktop's palette; `mod.rs` derives the colors drawn from it |
| `src/image/` | The data model: `Samples`, `color/` (transfer functions, primaries, ICC and CICP), stats, display state; `sequence.rs` is what a file holds beyond one image |
| `src/image/decode/` | The decoder trait and its registry, one file per format |
| `src/image/encode.rs` | The display pipeline run over every pixel, out to an 8-bit sRGB PNG |
| `src/render/` | Upload planning, the three layers, output selection; `shader_codes.rs` is every integer the shaders switch on |
| `src/render/reduce.rs` | The coarse chain a minifying draw reads from |

