### Web and everyday

| Format | Notes |
| --- | --- |
| JPEG | ICC profiles; HDR gain maps; Google depth maps; orientation; CMYK |
| PNG | 8 and 16-bit; indexed; alpha; PQ and HLG, ICC, gamma and primaries; animation |
| WebP | Lossy and lossless; alpha; ICC profiles; orientation; animation |
| GIF | Animation |

### Photography

| Format | Notes |
| --- | --- |
| Camera raw | Every major make and DNG; camera white balance and color into Rec. 2020, no curve or sharpening; toggle to camera's JPEG  |
| iPhone ProRAW | JPEG XL or lossless JPEG; rendered with iPhone profile or linear; each stage traced in the histogram |
| HEIC | Color space; 10 and 12-bit; iPhone HDR gain maps; portrait depth in meters |

### HDR

| Format | Notes |
| --- | --- |
| JPEG, HEIC with gain maps | Lifted within monitor's headroom  |
| iPhone ProRAW | Profile rendering as a gain map |
| PNG, JPEG XL, AVIF, HEIC | PQ and HLG; JPEG XL float kept above white |
| OpenEXR | Half and full float; RLE, ZIP, PIZ, PXR24, B44; metered exposure, neutral tone map |
| Radiance HDR | Linear float; metered exposure; header `EXPOSURE=` honored |

### Data

| Format | Notes |
| --- | --- |
| TIFF / BigTIFF | 8 to 64-bit integer or float, never rescaled; false color ramps; no-data values excluded; GeoTIFF latitude and longitude; multi-page |

### Other

| Format | Notes |
| --- | --- |
| AVIF | Stated color space; 10 and 12-bit |
| ICO | Multiple icons shown as pages |
| BMP | 1 to 32-bit; palettes; RLE |
| Netpbm | PBM, PGM, PPM, PAM; ASCII or binary; 8 or 16-bit |
