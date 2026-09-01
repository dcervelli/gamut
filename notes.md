# Notes

## THEMES

* GPU powered to the extent possible
* Image comparison (same image size) is first class
* Limited by CPU/GPU RAM
* No main thread work
* Performance is a differentiator

## INITIAL RELEASE
* Full UI pass
  * Settings icon LR for cursor interaction
  * Color spectrum chooser
  * Icons have px shifts
  * Scaling chooser
  * Toast for messages ("Copied filename")
* Histogram
  * Vertical scale
  * Log/linear
  * EV slider
* Region selection
* Copy
  * Region (raw)
  * Pixel color
* Modifications
  * 90° rotations
  * Crop to region
* Paste image
* Hotkey choices
  * Keys for magnifications
* Rewrite history with @closedcontour.com email
* Docs
  * Usage

## FUTURE

* Settings
  * Pixel format
  * Geospatial format
  * Date time format
* Color scale: alpha mask
* Thumbnail integration
  * Background thumbnail generation (show that instead of waiting on prev/next)
* Loupe
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
* Info cleanup
  * Info / Specific tag renderers?
  * Perf on scroll up/down by drag
