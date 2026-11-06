The pixel readout at the bottom left shows where the pointer is and the value under it. Both can be copied:

| Action | Linux | macOS |
|--|--|--|
| Copy pixel value | `Ctrl+.` | `⌘.` |
| Copy coordinate | `Ctrl+Shift+.` | `⇧⌘.` |

A coordinate is copied as a bare pair, such as `1919,1079`.

### Readout Menu

Press `.` or click the dot icon to open the menu, then press the key for an item. This is effectively a key chord, `.` then the letter to change settings.

| Coordinate | Key | Description |
|--|--|--|
| Pixel | `x` | Column and row |
| Print | `p` | Distance from the top left when printed, for [files with a DPI](#print-size) |
| Projected | `e` | The map's georeferenced coordinates, for georeferenced files |
| Geographic | `g` | Latitude and longitude, for georeferenced files |
| Azimuth/Elevation | `a` | Direction from the camera, when the [camera's orientation](#horizon-grid) is known |

| Pixel value | Key | Description |
|--|--|--|
| Hex | `h` | Stored values as hex. Note: this includes >8-bit and float images. |
| Decimal | `d` | Stored values in the file's own units |
| Mapped | `m` | Values after the window, exposure and tone curve |
| Depth | `z` | Distance from the camera, from the [depth map](#depth-maps) |

`u` switches Print between inches and millimeters, and `s` switches Geographic between decimal degrees and degrees, minutes and seconds.

`,` steps through the coordinates without opening the menu.

The chosen coordinate also sets the units of the [grid](#grid) and of [region selection](#region-selection), so choosing Print or Projected measures them in print or map units. Because Geographic and Azimuth/Elevation are non rectangular, they instead use map units and pixels, respectively.
