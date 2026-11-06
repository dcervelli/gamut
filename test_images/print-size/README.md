# Print size

`ruler-150dpi.png` is not a decoder fixture: it is the picture the zoom
menu's **Print size** is checked against by eye. It says 150 dpi, and holds
an inch ruler and a millimeter ruler to six inches and the outline of a
credit card (ISO/IEC 7810 ID-1, 85.60 × 53.98 mm). At print size the rulers
measure true against a real one and a card held to the screen covers the
outline; a monitor that reports its size wrongly shows all three off by the
same factor.

`generate.sh` makes it, with ImageMagick. `image::exif`'s
`the_print_size_ruler_is_150_dpi` holds the file to the density it is drawn
at.
