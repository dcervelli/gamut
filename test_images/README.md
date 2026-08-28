# Decoder fixtures

Real files, written by ImageMagick, for testing the decode paths in
`src/image/decode/`. Round-tripping through the `image` crate's own encoder
only proves the crate agrees with itself; these exercise encodings it has to
read from the outside world.

Regenerate with `./generate.sh` — it is the authoritative description of how
each file was made. Most fixtures need only ImageMagick; the measurement
rasters at the end need GDAL.

## The pattern

Every image is 32×24, four 16×12 quadrants, probed at their centres:

|          | top-left | top-right | bottom-left | bottom-right |
| -------- | -------- | --------- | ----------- | ------------ |
| colour   | red      | green     | blue        | white        |
| grey     | 0        | 85        | 170         | 255          |
| alpha    | 255      | 191       | 128         | 64           |
| float    | 0.0      | 0.5       | 1.0         | 3.984        |

Because 85/255 and 21845/65535 are both exactly a third, one table of expected
values covers 8-bit and 16-bit fixtures alike. The float highlight sits above
1.0 on purpose, so HDR fixtures exercise the range that has to survive to tone
mapping.

## Coverage

| Format | Files |
| ------ | ----- |
| PNG | grey / grey+alpha / RGB / RGBA at 8 and 16 bits, 1- and 4-bit depths, palette, palette + `tRNS`, Adam7 interlacing |
| JPEG | baseline, greyscale, progressive, 4:2:0 subsampling |
| TIFF | grey / RGB / RGBA at 8 and 16 bits, 32-bit float, LZW / Deflate / PackBits / uncompressed, big-endian, tiled |
| TIFF as raster data | BigTIFF, Deflate + floating-point predictor + tiling (how DEMs ship), signed Int16, GDAL no-data sentinel |
| Radiance | RGBE with its shared exponent |
| OpenEXR | RGB, RGBA with associated alpha, zip compression |
| Routing | `mislabelled.tif` (a PNG, found by sniffing), `.jpeg` and `.tiff` spellings |
| Failure | `unsupported.gif`, `bad-truncated.png` |

`src/image/decode/fixture_tests.rs` asserts that this directory and its
fixture table stay in step, so a file cannot be added without a test and a
test cannot outlive its file.
