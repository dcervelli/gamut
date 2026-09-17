# gamut

A modern Linux image viewer optimized for getting work done, fast.

![Main screenshot](user-docs/screenshots/main_screenshot.jpg)

## Features

- [Versatile Controls (Keyboard, UI, CLI)](#versatile-controls-keyboard-ui-cli)
- [OS themed](#os-themed)
- [Region selection/measurement](#region-selectionmeasurement)
- [Copy image](#copy-image)
- [Paste image](#paste-image)
- [Pixel info](#pixel-info)
- [Pixel grid](#pixel-grid)
- [Single channel false color](#single-channel-false-color)
- [Histogram and basic level manipulation](#histogram-and-basic-level-manipulation)
- [HDR](#hdr)
- [Fuzzy file navigation](#fuzzy-file-navigation)
- [Animated/multi-image formats](#animatedmulti-image-formats)
- [Color management](#color-management)
- [File comparison](#file-comparison)
- [File/directory watch](#filedirectory-watch)
- [Metadata/EXIF extraction](#metadataexif-extraction)
- [Many formats](#many-formats)
- [Fast GPU display](#fast-gpu-display)
- [Desktop/shell integration](#desktopshell-integration)

### Versatile Controls (Keyboard, UI, CLI)

All features can be keyboard driven but the hideable UI also contains discoverable controls with tooltips to learn the keyboard shortcuts. Controls can also be set via CLI flags.

### OS themed

Respects OS theme colors. Here's an example of Omarchy's Tokyo Night and Gruvbox themes.

![Themes side by side](user-docs/screenshots/themes.gif)

*Omarchy note:* Use the following window rule:

```lua
o.window("com.dcervelli.gamut", { float = true, center = true, tag = "-default-opacity", opacity = "1 1" })
```

This will make the window open centered and floating with a reasonable, dynamic default size. For accurate image viewing, the window should be forced fully opaque.

### Region selection/measurement

Select rectangular regions for measurement or copy. Pixel precision controls to move, grow, or shrink the region.

### Copy image

Images can be copied in a variety of useful ways:

* As a filename, with or without the path.
* As a file URI, for copy/paste in a file explorer.
* As an image/png, for copy/paste as an image. Useful for Slack, etc. Copied images have the current settings applied.

### Paste image

Clipboard images can be pasted and saved to the standard pictures folder as `pasted_${date}.png`.

### Pixel info

Get coordinate and color information for the moused-over pixel. Easily copy either the coordinate or color (in a variety of formats).
q
![Pixel copying](user-docs/screenshots/pixel_copy.gif)

### Pixel grid

Scale-dynamic pixel grid.

![Pixel grid animation](user-docs/screenshots/pixel_grid.gif)

### Single channel false color

Grayscale, 16-bit and float single-channel images can be shown with various color maps.

![False color maps](user-docs/screenshots/false_color.gif)

### Histogram and basic level manipulation

Luminance and per-channel histogram, with a linear or log count axis. Exposure in quarter stops, a window that slides and narrows, automatic windows (0–1, min/max, 99.8%), and tone mapping (none, Reinhard, neutral) for highlights above white. Nothing re-decodes; the histogram panel holds the controls.

### HDR

When available on the monitor, HDR sources are shown at their graded brightness. Tone mapped for SDR.

### Fuzzy file navigation

`ctrl+p` style navigation over the file list with thumbnails and helpful metadata per row. Thumbnails integrated into desktop cache.

![Fuzzy finder](user-docs/screenshots/fuzzy_finder.gif)

Bird images from [Fugleramme](https://github.com/arnegiacomo/fugleramme), augmented to include common name in the metadata.

### Animated/multi-image formats

Play animated GIF, PNG, WebP and JPEG XL files at normal speed or frame-by-frame. View multi-page TIFFs and ICOs.

![Animated GIF playback](user-docs/screenshots/animated.gif)

### Color management

Untagged files are treated as sRGB. ICC profiles (sRGB, Display P3, BT.2020 and Adobe RGB primaries, power-law tone response) and CICP tags are honored; PQ and HLG are decoded. 16-bit, float and single-channel data stay as they are rather than being flattened to 8-bit RGB. `--transfer` and `--primaries` override what a file says or fails to say.

### File comparison

Navigating between images with the same dimensions maintains pan and zoom making detailed image comparison straightforward.

This example compares a zoomed in region across DEM, hillshade, and relief images.

![File comparison example](user-docs/screenshots/compare.gif)

### File/directory watch

Files and directories are watched for changes, additions, or deletions. 

In this example, a simple program is zooming into a point on the Mandelbrot set and updating an image every second.

![Mandelbrot zoom via File Update](user-docs/screenshots/mandelbrot.gif)

### Metadata extraction

Get file, image, EXIF, XMP, georeference, and other metadata. Easily copy all, by section, or by item.

![Image info/metadata/EXIF](user-docs/screenshots/info.jpg)

### Many formats
PNG, JPEG (with gain maps), JPEG XL, TIFF and BigTIFF, WebP, HEIF (HEIC and AVIF), GIF, ICO, BMP, netpbm, Radiance HDR and OpenEXR. More details in [`user-docs/FORMATS.md`](user-docs/FORMATS.md).

### High performance

All rendering is done on GPU and maintains full frame rate. Decoding and statistics generation is parallelized in background threads.

### Desktop/shell integration

Registers as a handler for every format it reads, so it appears in file managers' "Open With" lists, and its own open button offers every other installed program that claims the file's type, read from the desktop's own MIME index. Ships a man page and bash, fish and zsh completions. Thumbnails are shared with the file manager through the freedesktop cache.

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
| Space | Cycle fit → fill → 100% |
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
