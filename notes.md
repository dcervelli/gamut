README:
* GPU powered to the extent possible
* Image comparison is first class
* Limited by CPU/GPU RAM
* No main thread work

TODO:

* Grid display when zoomed in
* Cursor color tracking
* Region selection
  * Nudges
* Crop to region
* Copy
  * File path
  * Image (file content)
  * Image (raw)
  * Region (raw)
  * Pixel color
* Changable background
  * Color picker
* Hide UI key
* Sane keys 
* Formats
  * GIF
  * BMP
  * DNG
  * Other raw files
  * Animated GIF support
  * SVG rasterizer?
* Scan directory for image with/without -r
* If window is small, auto hide side panels
* Basic modifications
* Wayland app_id
* Security pass
* Info
  * EXIF
  * TIFF tags
  * Other?
* UI theme colors

UI:

* For the filename, if all files are in the same directory, just display the file name; if not, display a minimum disambiguated file path.
* As the mouse moves around the image, display the pixel coordinate of the mouse, or "-, -" if not in bounds.
