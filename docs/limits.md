# Known limits

Decoding is synchronous: the window does not appear until the first image is
ready, and switching files blocks until the next one is. Mount Rainier at 3 m
(18333 × 15667, 589 MB of Deflate) takes about six seconds and peaks around
1.4 GB of resident memory against a 1.15 GB decoded buffer. Nothing is
streamed to the GPU in tiles, so an image also has to fit in one texture.

EXIF orientation is not applied, so a rotated phone JPEG shows unrotated. HEIF
and WebP are the exceptions: HEIF's rotation lives in the container rather than
in a metadata tag, and WebP's tag sits in a chunk its decoder already opens for
the color profile.

A HEIF holding several images — a burst, a Live Photo, an AVIF image sequence
— shows the one it marks as primary. The `libheif` binding exposes no
sequence API, so the others are not reachable the way an ICO's entries are.
An EXR shows its first layer for the same kind of reason: `image` hands back
one.

A 16-bit animated PNG shows its default image and does not play: `image`
composites APNG frames at eight bits and refuses a deeper file. A TIFF's
directories are all pages, so a pyramid's overviews and a file's thumbnail
show as pages beside the picture; telling them apart by `NewSubfileType` is
not done. Everything about how an animation is played is in
[animation.md](animation.md).

Embedded ICC profiles are read for JPEG, PNG, HEIF and WebP — and so for an
ICO whose entry is a PNG — which is every format here that can carry one
except TIFF, which can and does not. GIF has no way of carrying one.

Gain maps are read for JPEG only. HEIF can carry one as an auxiliary image,
which is how Apple stores HDR photographs, and that is not implemented; such a
file shows its SDR base. The obstacle is not the arithmetic, which is the same
code the JPEG path already runs, but the metadata: `libheif` 1.23 exposes no
gain map API at all, so reaching it would mean either walking the ISOBMFF
boxes for an ISO 21496-1 `tmap` item or reverse-engineering Apple's maker
note. Worth revisiting when `libheif` exposes it. A gain map running the other
way — where the stored image is the HDR one — is refused rather than applied,
since applying it backwards would brighten what was already bright.

Reconstruction costs memory: the result is four 32-bit floats per pixel, so a
12-megapixel photograph is a 200 MB buffer where the base image alone was 12
MB, with a transient copy of the same size on the way out of the gain map
crate.

