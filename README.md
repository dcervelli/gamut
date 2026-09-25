# gamut

A modern Linux image viewer for understanding and acting on images.

![Main gamut screenshot](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/main_screenshot.jpg)

Image viewers tend toward minimalism or full-fledged editors; gamut stakes a claim in the middle. It's for when you need to *see* the image, *understand* it, and *decide* what you're going to do with it: check for sharpness, assess a render, compare similar images, pull a pixel value or coordinate, extract some metadata, copy a region to chat, delete it outright, or send it to a domain-specific editor.

An image viewer serves many audiences: photographer, data scientist, programmer, designer, game developer, GIS analyst, and, of course, the casual user. We carefully consider the tenets below in deciding if a feature clears the bar for inclusion.

## Tenets

* *Fast, always*. Every pan and zoom runs at full frame rate; decoding and analysis never block the interface. Speed is never traded for a feature.
* *Faithful to the file*. Color management; HDR as graded; 16-bit, float, single-channel data displayed as-is. The file's actual pixel values are always a hover away.
* *Viewer, not editor*. You can rename, trash, copy, and export, but nothing changes the file's pixels. Adjustments are for seeing the image better, and what you see is what you copy or export.
* *Useful, not minimal*. If a feature is conceptually simple but would otherwise require specialized software, it has a place.
* *Images come first*. The interface is sparse, and every image, primary and thumbnail, gets as much room as possible. Specialized tools stay out of sight unless asked for.
* *Discoverable by mouse, driven by keyboard*. Every feature is accessible through the UI and keyboard. Learn the keys via tooltips and in-app help.
* *Desktop native*. Integrates with your theme, trash, thumbnail cache, file dialogs, and other image applications.

## Features

- [High performance](#high-performance)
- [Many formats](#many-formats)
- [Animated/multi-image formats](#animatedmulti-image-formats)
- [Versatile controls (Keyboard, UI, CLI)](#versatile-controls-keyboard-ui-cli)
- [Metadata extraction](#metadata-extraction)
- [Copy/paste image](#copypaste-image)
- [Region selection/measurement](#region-selectionmeasurement)
- [Pixel info](#pixel-info)
- [Pixel grid](#pixel-grid)
- [Loupe](#loupe)
- [Histogram](#histogram)
- [HDR](#hdr)
- [Color management](#color-management)
- [Single channel false color](#single-channel-false-color)
- [Fuzzy file navigation](#fuzzy-file-navigation)
- [Filmstrip](#filmstrip)
- [File comparison](#file-comparison)
- [File/directory watch](#filedirectory-watch)
- [Export](#export)
- [Desktop/shell integration](#desktopshell-integration)
- [OS themed](#os-themed)

### High performance

All rendering is done on GPU and maintains full frame rate through both user initiated and (rapidly) animated pans and zooms. Decoding and statistics generation is parallelized in background threads.

### Many formats

PNG, JPEG (with gain maps), JPEG XL, TIFF and BigTIFF, WebP, HEIF (HEIC and AVIF), GIF, ICO, BMP, raw camera files, netpbm, Radiance HDR and OpenEXR. More details in [`user-docs/FORMATS.md`](user-docs/FORMATS.md).

Feel free to submit a PR or open an issue for additional file formats.

### Animated/multi-image formats

Play animated GIF, PNG, WebP and JPEG XL files at normal speed or frame-by-frame. View multi-page TIFFs and ICOs.

![Animated GIF playback](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/animated.gif)

### Versatile controls (Keyboard, UI, CLI)

All features can be keyboard driven but the hideable UI also contains controls with tooltips to learn the keyboard shortcuts. Controls can also be set via CLI flags.

![UI controls](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/ui.gif)

### Metadata extraction

Get file, image, EXIF, XMP, georeference, and other metadata. Easily copy all, by section, or by item.

![Image info/metadata/EXIF](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/info.jpg)

### Copy/paste image

Images can be copied in a variety of useful ways:

* As a filename, with or without the path.
* As a file URI, for copy/paste in a file explorer.
* As an image/png, for copy/paste as an image. Useful for Slack, etc. Copied images have the current settings applied.

Clipboard images can be pasted and saved to the standard pictures folder as `pasted_{date}.{extension}`.

### Region selection/measurement

Select rectangular regions for measurement or copy. Pixel precision controls to move, grow, or shrink the region. This example shows drawing a rough box then selecting handles and using keys to get it exact.

![Region selection](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/region.gif)

### Pixel info

Get coordinate and color information for the moused-over pixel. Easily copy either the coordinate or color (in a variety of formats).

![Pixel copying](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/pixel_copy.gif)

### Pixel grid

Scale-dynamic pixel grid.

![Pixel grid animation](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/pixel_grid.gif)

### Loupe

An easily triggerable loupe for quickly examining details.

![Loupe](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/loupe.gif)

### Histogram

A dynamic histogram primarily for inspecting the content of the image. While you can make some adjustments to better understand the content of the image, gamut is a [viewer, not an editor](#tenets).

![Histogram](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/histogram.gif)

### HDR

When available on the monitor (`--output hdr` attempts to force the display into HDR), HDR sources are shown at their graded brightness.

### Color management

Untagged files are treated as sRGB. ICC profiles (sRGB, Display P3, BT.2020 and Adobe RGB primaries, power-law tone response) and CICP tags are honored; PQ and HLG are decoded. 16-bit, float and single-channel data are not flattened to 8-bit RGB. 

### Single channel false color

Grayscale, 16-bit and float single-channel images can be shown with various color maps.

![False color maps](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/false_color.gif)

### Fuzzy file navigation

`ctrl+p` style navigation over the file list with thumbnails and helpful metadata per row. Thumbnails are integrated into the desktop cache.

![Fuzzy finder](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/fuzzy_finder.gif)

Bird images from [Fugleramme](https://github.com/arnegiacomo/fugleramme), augmented to include common name in the metadata.

### Filmstrip

The file list is a filmstrip-like view for navigating through your images visually. 

![Filmstrip](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/file_list.gif)

### File comparison

Navigating between images with the same dimensions maintains pan and zoom making detailed image comparison straightforward.

This example compares a zoomed in region across DEM, hillshade, and relief images.

![File comparison example](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/compare.gif)

### File/directory watch

Files and directories are watched for changes, additions, or deletions. 

In this example, a simple program is zooming into a point on the Mandelbrot set and updating an image every second. gamut updates as soon as the file changes and maintains pan/zoom settings across reloads.

![Mandelbrot zoom via File Update](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/mandelbrot.gif)

### Export

A basic file export to lossless PNG or JPG with an option to resize.

![Export](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/export.gif)

### Desktop/shell integration

* [Registers as a handler](packaging/com.dcervelli.gamut.desktop) for all formats it reads as well as directories;
* man page;
* bash, fish, and zsh completions;
* "Open in…" menu:
![Open in… menu](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/open_in.jpg)

### OS themed

Respects OS theme colors. Here's an example of Omarchy's Tokyo Night and Gruvbox themes.

![Themes side by side](https://raw.githubusercontent.com/dcervelli/gamut-scripting/master/screenshots/themes.gif)

*Omarchy note:* Use the following window rule:

```lua
o.window("com.dcervelli.gamut", { float = true, center = true, tag = "-default-opacity", opacity = "1 1" })
```

This will make the window open centered and floating with a reasonable, dynamic default size. For accurate image viewing, the window should be forced fully opaque.

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
| `tab` | Show/hide the file list |
| `q`, `esc` | Quit |

All controls are documented in [`user-docs/KEYS.md`](user-docs/KEYS.md). CLI help is available via `gamut --help`.

## Contributing

Issues and pull requests are welcome. See [Roadmap](ROADMAP.md) for some features and improvements that we'd like to get to.

[`docs/`](docs/) — AI generated and maintained notes on development.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. 

[Lucide](https://lucide.dev) icon geometries licensed under ISC.

Polynomial false-color ramps are CC0 or Apache-2.0.
Further information in [`docs/licensing.md`](docs/licensing.md).
