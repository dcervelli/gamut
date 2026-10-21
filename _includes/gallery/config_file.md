At startup (and with live reload, to the extent possible), gamut reads `~/.config/gamut/config` (the same path on Linux and macOS). You can easily open this file (and create one if it doesn't exist) using the button at the bottom of the help menu.

The config file allows you to adjust settings, keybindings, and gestures. CLI flags supersede config values.

gamut also manages a state file: `~/.local/state/gamut/state` on Linux, `~/Library/Application Support/gamut/state` on macOS.

This is the portion of the current default config not involved with keybindings and gestures:

```
# The panels around the picture; ` hides and shows them.
# show_ui = true

# The minimap, while the picture is larger than the window.
# show_minimap = true

# The file list down the left, while there is more than one file.
# show_filmstrip = true

# The histogram, in the side panel; it wins over show_info where both are on.
# show_histogram = false

# The file information, in the side panel.
# show_info = false

# A single file opened alone steps on through the other images in its folder.
# browse_folder = true

# The web page the map buttons in the info panel's Location and Georeference sections open: {lat} and {lng} are where the picture was taken, or the middle of what it covers, in degrees.
# open_map_link = https://geojson.io/#data=data:application/json,%7B%22type%22%3A%22Feature%22%2C%22properties%22%3A%7B%7D%2C%22geometry%22%3A%7B%22type%22%3A%22Point%22%2C%22coordinates%22%3A%5B{lng}%2C{lat}%5D%7D%7D

# The web page the corners button in the info panel's Georeference section opens: {lat_ul} and {lng_ul}, {lat_ur} and {lng_ur}, {lat_lr} and {lng_lr}, and {lat_ll} and {lng_ll} are the picture's upper left, upper right, lower right and lower left corners, in degrees.
# open_map_bounds_link = https://geojson.io/#data=data:application/json,%7B%22type%22%3A%22Feature%22%2C%22properties%22%3A%7B%7D%2C%22geometry%22%3A%7B%22type%22%3A%22Polygon%22%2C%22coordinates%22%3A%5B%5B%5B{lng_ll}%2C{lat_ll}%5D%2C%5B{lng_lr}%2C{lat_lr}%5D%2C%5B{lng_ur}%2C{lat_ur}%5D%2C%5B{lng_ul}%2C{lat_ul}%5D%2C%5B{lng_ll}%2C{lat_ll}%5D%5D%5D%7D%7D

# The program the info panel's Raw Data tab reads every tag with: a name looked for on the PATH, or a path.
# exiftool = exiftool
```
