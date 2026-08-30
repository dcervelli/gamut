# Notes

## THEMES

* GPU powered to the extent possible
* Image comparison (same image size) is first class
* Limited by CPU/GPU RAM
* No main thread work
* Performance is a differentiator

## INITIAL RELEASE

* Full UI pass
  * Cursor swatch to right of coordinate
* Theme font
* License review
* Minimap on by default
* Security pass with Fable
* Grid display when zoomed in
* Region selection
* Copy
  * File path
  * Image (file content)
  * Image (raw)
  * Region (raw)
  * Pixel color
* Paste image
* Hotkey choices
* Modifications
  * 90° rotations
  * Crop to region
  * Gamma
* Wayland app_id
* Package
* Info
  * File date/time
  * Resolution
  * File size
  * Color
  * EXIF
  * TIFF tags
  * Other?
* Docs
  * Usage
  * CLI
  * Man page

## FUTURE

* Export/Save as...
* Multiframe
  * GIF
  * WebP
  * ICO
* --recursive directory scan
* File list/filmstrip
* Ctrl+P to open file
* Clean steps on zoom
* Logging
  * ICO → largest image chosen
  * Errors
* Changable background
  * Color picker
* SVG rasterizer
* For the filename, if all files are in the same directory, just display the file name; if not, display a minimum disambiguated file path.
* Formats
  * JPEG XL
  * DNG
  * Other raw files
