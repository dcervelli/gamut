# image-view

A small GPU-accelerated image previewer: Rust, `winit` for the window, `wgpu`
for the drawing, `glyphon` for the status bar.

## Build and run

```sh
cargo run --release -- photo.jpg scan.tiff diagram.png
```

The first file is shown, stretched to fit the window. The window opens at the
image's own size, shrunk to fit comfortably on the monitor.

## Keys

| Key | Action |
| --- | --- |
| `q`, `Esc` | Quit |
| `+`, `=` | Zoom in |
| `-`, `_` | Zoom out |
| `0` | Actual size (100%) |
| Arrows | Pan |
| `f` | Cycle fit → fit width → fit height |
| `n`, `p` | Next / previous file |

Zooming leaves fit mode; panning does not, so `f` then Down scrolls through a
tall image at fit-width. Panning stops at the image's edges, and an image
smaller than the window stays centred.

Keys pressed with Ctrl, Alt or Super are ignored, so window-manager chords such
as `Super+0` do not disturb the view.

## Formats

PNG, JPEG and TIFF, decoded by the [`image`](https://crates.io/crates/image)
crate. The decoder is chosen by file extension, falling back to content
sniffing when the extension is missing or wrong.

### Adding a format

Formats live behind one trait, `formats::Decoder`:

```rust
pub trait Decoder: Sync {
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn sniff(&self, header: &[u8]) -> bool;
    fn decode(&self, bytes: &[u8]) -> Result<DecodedImage>;
}
```

To add one:

1. Write a module in `src/formats/` implementing `Decoder`. `decode` returns a
   `DecodedImage`: 8-bit non-premultiplied RGBA, top row first, tightly packed.
   Everything downstream — texture upload, fit maths, the status bar — already
   works in those terms.
2. Add your decoder to the `DECODERS` slice in `src/formats/mod.rs`.

That is the whole change; nothing in the renderer, the view or the event loop
needs to know the format exists. `--help` and the "unsupported image format"
message pick up the new extensions automatically, and the
`every_extension_is_claimed_by_exactly_one_decoder` test guards against two
decoders fighting over the same extension.

## Layout

| File | Role |
| --- | --- |
| `src/main.rs` | Argument parsing, event-loop setup |
| `src/app.rs` | Window lifecycle, key handling, status line |
| `src/view.rs` | Zoom / pan / fit geometry — pure maths, unit tested |
| `src/formats/` | The decoder trait and its registry |
| `src/renderer/` | wgpu setup, the per-frame draw, the text overlay |

## Tests

```sh
cargo test
```

Covers the view geometry and the decoder registry. The rendering itself is not
covered by tests; it needs a GPU and a window.
