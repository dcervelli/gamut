# gamut

A modern Linux image viewer optimized for getting work done, fast.

![Main screenshot](user-docs/screenshots/main_screenshot.jpg)

## Features

### Versatile Controls (Keyboard, UI, CLI)

All features can be keyboard driven but the hideable UI also contains discoverable controls with tooltips to learn the keyboard shortcuts. Controls can also be set via CLI flags.

### OS themed

Respects OS theme colors. Here's an example of Omarchy's Lupine and Tokyo Night themes.

![Themes side by side](user-docs/screenshots/themes.gif)

*Omarchy note:* Use the following window rule:

```lua
o.window("com.dcervelli.gamut", { float = true, center = true, tag = "-default-opacity", opacity = "1 1" })
```

This will have the window open centered and floating with a reasonable and dynamic default size. For accurate image viewing, the window should be forced fully opaque.

### Copy image

Images can be copied in a variety of useful ways:

* As a filename, with or without the path.
* As a file URI, for copy/paste in a file explorer.
* As an image/png, for copy/paste as an image. Useful for Slack, etc. Copied images have the current settings applied.

### Paste image

Clipboard images can be pasted and saved to the standard pictures folder as `pasted_${date}.png`.

### Pixel info

Get coordinate and color information for the moused-over pixel. Easily copy either the coordinate or color (in a variety of formats).

### Pixel grid

Scale-dynamic pixel grid.

![Pixel grid animation](user-docs/screenshots/pixel_grid.gif)

### Single channel false color

### Histogram and basic level manipulation

### HDR

### Color management

### File comparison

### File/directory watch

### Metadata/EXIF extraction

Get file, image, EXIF, georeference, and other metadata. Easily copy all, by section, or by item.

![Image info/metadata/EXIF](user-docs/screenshots/info.jpg)

### Many formats

PNG, JPEG (with gain maps), JPEG XL, TIFF and BigTIFF, WebP, HEIF (HEIC and
AVIF), GIF, ICO, BMP, netpbm, Radiance HDR and OpenEXR. 
More details in [`user-docs/FORMATS.md`](user-docs/FORMATS.md).

### Fast GPU display

### Desktop integration

## Install

```sh
cd packaging && makepkg -si
```

## Getting Started

Just pass filenames or directories to `gamut`:
```
gamut file1.jpg pictures file2.png
```
Directories are scanned non-recursively.

Enough keys to get going:

| Key | |
| --- | --- |
| `]`, `[` or `PgDn`, `PgUp` | Next / previous file |
| Space | Cycle fit → fit width → fit height |
| `1` | Actual size; wheel to zoom, drag to pan |
| `d`, `f` | Exposure down / up |
| `h`, `i`, `m` | Histogram, file information, minimap |
| `` ` `` | Hide the interface |
| `q` | Quit |

All controls are documented by the AI in [`user-docs/KEYS.md`](user-docs/KEYS.md). Every display control is also a
start-up flag, which `gamut --help` lists.

## Roadmap

See [Roadmap](ROADMAP.md).

## Contributing

Issues and pull requests are welcome.

[`docs/`](docs/) — AI generated and maintained notes on development.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. 

[Lucide](https://lucide.dev) icon
geometries licensed under ISC.

Polynomial false-color ramps are CC0 or Apache-2.0.
Further information in [`docs/licensing.md`](docs/licensing.md).
