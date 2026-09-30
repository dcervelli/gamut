# The information panel

`i` or the info button opens the panel down the right of the window. It
lists what the file says about itself, section by section, in the order
below. A field the file does not fill in is left out, and so is a section
with nothing in it. A click on a field copies its value; a click on a
section's heading copies the whole section as CSV; **Copy All** at the top
copies everything, one line per field, as `Ctrl+I` does.

EXIF is read from JPEG, TIFF, PNG, WebP and HEIF files, and XMP from those
and from JPEG XL. An XMP sidecar beside any file, raws included — the file's
name with `.xmp` in place of its extension, or added after it — is read
too, and what it says wins over the file's own XMP.

## File

Headed by the file's name. A long name is cut in the middle and keeps its
extension.

| Field | What it says |
| --- | --- |
| Folder | The folder the file is in, as the path was given. Left out for a file named on the command line without one. |
| Size | How large the file is on disk, rounded, with the exact count in the tooltip. |
| Modified | How long ago the file was last written, with the date in UTC in the tooltip. |

## About

Headed by the file's title, or by *About* where it has none. Only what
somebody wrote about the picture. Where the EXIF and the XMP both have a
field, the EXIF one is shown.

| Field | Where it comes from |
| --- | --- |
| Title | XMP only: EXIF has no field for it. |
| Caption | EXIF's image description, or XMP's description. |
| Comment | EXIF's user comment. |
| Artist | EXIF's artist, or XMP's creators. |
| Keywords | XMP only. |
| Copyright | EXIF's copyright, or XMP's rights. |

## Image

Headed by the picture's size and the format that read it — "4000 × 3000
JPEG". While the depth map is shown in the picture's place, this section
describes the map.

| Field | What it says |
| --- | --- |
| Resolution | Width × height, after the file's orientation is applied. A turn made with the keys is not counted. |
| Read by | The format the file turned out to be, by its contents rather than its extension. |
| Rendering | *camera JPEG* while a raw's embedded JPEG is shown instead of the developed picture. |
| Showing | *depth map* while the depth map is shown in the picture's place. |
| Orientation | The turn the file's EXIF orientation asks for, which has already been applied. |
| Compression | TIFF only: how its pixels are compressed. |
| Samples | Bits per sample and the channel layout: *8-bit RGB*, *32-bit float gray*. |
| Precision | Only when the GPU could not hold the picture at full precision, and why; it is then shown as half float. |
| Color space | The color space the numbers are in, by its common name where it has one: *sRGB*, *Display P3*. |
| Alpha | Straight or premultiplied, only where there is an alpha channel. |
| Referred to | Whether the numbers are graded (1.0 is white), scene light, or a measurement. This decides how the picture first opens. |
| Gain map | Whose description of the gain map the file gives: ISO 21496-1 or Apple. |
| Gain map size | The gain map's size, and whether it is one channel or three. |
| HDR headroom | How far above SDR white the gain map can lift the picture, in stops. |
| Gain applied | How much of that lift is on screen. *none* on a display with no room above white. |
| Exposure applied | Radiance HDR only: the multiplier the file says was already applied. |
| Holds | For an animation, how many frames and how it loops; for a file of pages, how many and which one this is. |

## Depth map

Only for a picture that carries a depth map: an iPhone portrait, or a JPEG
with Google's depth data. It describes the map whether the picture or the
map is on screen.

| Field | What it says |
| --- | --- |
| Resolution | The map's own size, usually much smaller than the picture's. |
| Samples | Bits per sample of the map. |
| Described by | Apple or Google: whose description of the map the file carries. |
| Encoding | *distance* or *inverse distance*, which is what the map's numbers are spread over. *unknown* where the file does not say; the pixel readout then shows the raw numbers. |
| Range | The nearest and farthest distances the map can express. |
| Accuracy | *absolute*, or *relative* where the file says the distances are only estimated. An iPhone's dual-camera portrait is relative: nearer and farther are right, but the distances themselves are not, and are marked `≈`. Google's depth data does not say, and is shown as absolute. |

## Camera

Headed by the camera's make and model. For a camera raw, anything the EXIF
leaves out is filled in from what the raw reader finds in the file.

| Field | What it says |
| --- | --- |
| Lens | The lens's name, with its maker in front where that is not the camera's maker. Where the file gives no name, its range of focal lengths and apertures. |
| Owner | The owner's name, as set in the camera. |
| Camera serial number | |
| Lens serial number | |

## Exposure

| Field | What it says |
| --- | --- |
| Taken | How long ago the picture was taken, with the date in the tooltip: the camera's clock, with its offset from UTC where it recorded one. |
| Mode | Manual, Program, Aperture priority and so on, and *Auto bracket* where the shot was one of a bracket. |
| Shutter speed | |
| Aperture | |
| ISO | |
| Exposure compensation | Left out when it is zero. |
| Focal length | With its 35 mm equivalent, and any digital zoom. |
| Metering | How the camera measured the light. |
| White balance | Auto or Manual. |
| Color temperature | Camera raw only: the temperature the white balance was set for. It is worked out from the camera's own color data, and is not in the EXIF. |
| Flash | Whether it fired and what it was set to. Left out for a camera with no flash. |
| Composite | Only where the camera merged several frames into the picture. |

## Location

Headed by the latitude and longitude in degrees, with the map button after
them. The map button opens where the picture was taken in the browser; the
`open_map_link` setting in [SETTINGS.md](SETTINGS.md) picks the site.

| Field | What it says |
| --- | --- |
| Altitude | In meters; negative below sea level. |
| Direction | Which way the camera faced, in degrees from true or magnetic north. The two can be twenty degrees apart. |
| Speed | How fast the camera was moving, in km/h, mph or knots, as the file records it. |
| Positioning error | How far off the recorded position may be, as the GPS judged it. |

## Georeference

Only for a raster that says where its pixels are on the ground, such as a
GeoTIFF elevation model or scanned map.

| Field | What it says |
| --- | --- |
| Coordinate system | The projection or the geographic system the coordinates are in, by name and EPSG code as the file gives them. They are not looked up, so what you see is what was written. |
| Vertical system | What the heights are measured from. |
| Pixel is | Whether a coordinate is a pixel's corner or its center. |
| Pixel size | How much ground one pixel covers, in the coordinates' unit. |
| Origin | The coordinates of the top left corner. |
| Min easting, Max easting, Min northing, Max northing | The ground the raster covers. For geographic coordinates these are longitude and latitude. |
| Orientation | Only where the raster is rotated against the map's axes. |
| No data | The value that marks a pixel with no data. |

## Regions

The regions the file marks out on the picture, as a table of their subject,
their top left corner (X, Y) and their size (W, H). Positions are in the
picture's pixels as shown, so they follow the orientation and any turn. Rest
the pointer on a row to outline that region on the picture, or on the
heading to outline them all. A row copies as a line of CSV.

The regions come from:

- The subject the camera found, from EXIF.
- The regions a cataloging program wrote into the XMP, usually faces, in the
  Metadata Working Group's form.
- The people tags Windows Photo Gallery wrote. A face tagged both ways is
  shown once.

The subject column names who is in the region, or what kind of region it is
where nothing says. Anything else written about it, such as a description,
a barcode's value or whether it was a focus point, is in its tooltip.
