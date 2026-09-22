# Known limits

A file is decoded whole, on the loader thread. Mount Rainier at 3 m
(18333 × 15667, 589 MB of Deflate) takes about six seconds and peaks around
1.4 GB of resident memory against a 1.15 GB decoded buffer. Nothing is
streamed to the GPU in tiles, so an image also has to fit in one texture.

A HEIF holding several images — a burst, a Live Photo, an AVIF image sequence
— shows the one it marks as primary. The `libheif` binding exposes no
sequence API, so the others are not reachable the way an ICO's entries are.
An EXR shows its first layer for the same kind of reason: `image` hands back
one.

A 16-bit animated PNG shows its default image and does not play: `image`
composites APNG frames at eight bits and refuses a deeper file. Every TIFF
directory but a transparency mask is a page, so a pyramid's overviews and a
file's thumbnail show as pages beside the picture. Everything about how an
animation is played is in [animation.md](animation.md).

Embedded ICC profiles are read for every format here that can carry one —
JPEG, PNG, TIFF, HEIF, JPEG XL and WebP, and so an ICO whose entry is a PNG.
GIF has no way of carrying one.

Gain maps are read from a JPEG and from a HEIF; an AVIF with an ISO 21496-1
`tmap` goes the same way, untested for want of a file. A gain map running
the other way — where the stored image is the HDR one — is refused rather
than applied, since applying it backwards would brighten what was already
bright, and so is a map deeper than 8 bits. A HEIF's gain map is applied
only to an 8-bit color base: a 10-bit HEIC is HDR in its own right and
carries none.

Reconstruction costs memory: the result is four 32-bit floats per pixel, so a
12-megapixel photograph is a 200 MB buffer where the base image alone was 12
MB.

