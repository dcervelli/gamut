# Roadmap

# Bugs
* Open image -> don't finish decode, go second image -> resize to fit second image.

## New Features
* RGBA channel toggle. For gamedev people.
* SVG rasterizer. How to choose DPI correctly?
* >2^15 px images. Perhaps a separate program for large rasters? Requires image pyramids and other optimizations.
* Text extraction. Tesseract intergration that captures bounding boxes and allows visual text content extraction.
* Basic annotations? Not sure it should be included. If included, very basic: rectangle (filled/stroked), line, arrow, text box; nothing more.
* Mirror/flip image.
* Histogram (and other stats) and info panel should be toggles on the content of a right panel that doesn't overlap the image.
* Configurable executable shortcuts (example: `my_favorite_program %f` → ^1).
* More advanced file list/filmstrip features including a simple mark/flag and a few bulk operations.

## Improve
* Fixed aspect region selection
* Show in finder/explorer button
* Sort keyboard shortcuts
* Configurable paste directory
* Cascading app size growth?
* Icon shape and border
* Native fullscreen command (Linux)
* Allow panning off screen?
* Drop down for back/forward buttons
* Another security pass
* Histogram documentation
* Copy region handle
* Honor GDAL NO_DATA
* Histogram marker icons so they can't overlap
* Output histogram
* --recursive directory option
* Red-to-blue color spectrum / more spectra with a chooser
* Comparison mode toggle
* Error toasts
* Settings
  * Date time format
  * Changable background
* Color scale: alpha mask
