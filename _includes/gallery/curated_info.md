A curated set of image metadata is collected from the file. EXIF, XMP, RAW fields, and other sources are used to put together the following sections. All data is easily copyable and exportable and, in some cases, interactive. [Raw data](#raw-metadata) is also available.

## Sections

| File | Name, path, size and last modified date. Always shown. |
| About | Title, caption, comment, artist, keywords and copyright. Only shown if available from EXIF/XMP. |
| Image | Resolution, format, samples, color space, alpha, and more. Always shown. |
| Camera | Camera make and model, lens, owner, serial numbers and software. Shown if available. |
| Exposure | Capture time, mode, shutter speed, aperture, ISO, compensation, focal length, metering, white balance, flash. Shown if available. |
| Location | GPS and compass information. [Link to external map](#open-in-map). Shown if available. |
| [Orientation](#horizon-grid) | 3D representation of the camera in space. Hovering shows [Horizon Grid](#horizon-grid). Shown if available. |
| Gain map | Info about the gain map: format, HDR headroom, the lift, and how much is currently applied. See [HDR histogram](#hdr-histogram). Shown if available. |
| [Profile](#proraw-histogram) | The profile's baseline exposure, gain table and tone curve, and whether they are applied. Shown if available (currently just [ProRAW](#proraw-histogram)). |
| [Depth map](#depth-maps) | Size, sample info, encoding, range and accuracy. Shown if available. |
| Regions | A table of each region's subject, position and size; hovering a row shows the region on the image. |
| Georeference | Coordinate system info, pixel size and origin, ground extent, rotation and no-data value. [Link to external map](#open-in-map) (center or bounds). Shown for GIS files. |
