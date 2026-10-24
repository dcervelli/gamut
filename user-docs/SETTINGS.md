# Settings

`gamut` reads two files when it starts: the configuration file, which is
yours and which it reads again whenever you save it, and the state file,
which is its own. Neither has to exist.

## The configuration file

`~/.config/gamut/config` (under `$XDG_CONFIG_HOME` if you set it) says how the
window opens every time. It is in the same place on a Mac. `gamut` only reads it: toggling a panel in the window
lasts until the window closes and does not change the file.

Each line is `name = value`, and a `#` at the start of a line or after a space
starts a comment. Anything left out keeps its default. To start a file with every setting, every key and every mouse
gesture listed, commented out at its default, run:

```
mkdir -p ~/.config/gamut
gamut --print-config > ~/.config/gamut/config
```

Or press `Ctrl+,` (`⌘,` on a Mac), or **Edit configuration file** at the foot
of the help popup, which writes that file for you if there is none and opens
it — see [Editing it from the window](#editing-it-from-the-window).

Then remove the `#` from any line you want to change. A line you leave
commented out keeps following the default if a later version changes it. The
settings are:

| Setting | Default | What it does |
| --- | --- | --- |
| `show_ui` | `true` | The panels around the picture, which `` ` `` hides and shows |
| `show_minimap` | `true` | The minimap, while the picture is larger than the window |
| `show_filmstrip` | `true` | The file list, while there is more than one file |
| `show_histogram` | `false` | The histogram, in the panel down the right; it wins over `show_info` when both are on |
| `show_info` | `false` | The file information, in the panel down the right |
| `browse_folder` | `true` | A single file opened by itself steps on through the other images in its folder |
| `max_files` | `10000` | How many images opening folders may add to the list in one run. A folder opened with its subfolders stops being searched once this many have been found, and the window says so; images that later arrive in a folder already open still join |
| `exiftool` | `exiftool` | The program the info panel's *Raw Data* tab reads every tag with: a name looked for on your `PATH` and where packages put it, or a path to it |
| `open_map_link` | `https://geojson.io/#data=data:application/json,%7B%22type%22%3A%22Feature%22%2C%22properties%22%3A%7B%7D%2C%22geometry%22%3A%7B%22type%22%3A%22Point%22%2C%22coordinates%22%3A%5B{lng}%2C{lat}%5D%7D%7D` | The web page the map button in the info panel's *Location* section opens, and the pin button in its *Georeference* section. `{lat}` and `{lng}` become where the picture was taken, or the center of what a map covers, in degrees, negative south and west; the link must have both |
| `open_map_bounds_link` | `https://geojson.io/#data=data:application/json,%7B%22type%22%3A%22Feature%22%2C%22properties%22%3A%7B%7D%2C%22geometry%22%3A%7B%22type%22%3A%22Polygon%22%2C%22coordinates%22%3A%5B%5B%5B{lng_ll}%2C{lat_ll}%5D%2C%5B{lng_lr}%2C{lat_lr}%5D%2C%5B{lng_ur}%2C{lat_ur}%5D%2C%5B{lng_ul}%2C{lat_ul}%5D%2C%5B{lng_ll}%2C{lat_ll}%5D%5D%5D%7D%7D` | The web page the corners button in the info panel's *Georeference* section opens. `{lat_ul}` and `{lng_ul}` become the latitude and longitude of the picture's upper left corner, in degrees, negative south and west, and the same with `ur`, `lr` and `ll` for its upper right, lower right and lower left; the link must have at least one latitude and one longitude |

`--histogram`, `--info`, `--no-minimap` and `--alone` override the file for
that one run.
If a line has a misspelled name or a value `gamut` does not recognize, it
names the line on the terminal and uses the rest of the file. The window
says so too as it opens, naming the first such line and how many more there
are.

### Keys

Every key has a dotted name — `zoom.in`, `files.undo`, `region.move.left` —
listed beside it in [Keys and mouse](KEYS.md), and in the file
`--print-config` writes. A line binds the name to the keys after it,
separated by spaces, in place of its defaults:

```
keys.zoom.in = + =
keys.files.undo = ctrl+z
keys.files.export = ctrl+shift+e
keys.files.chooser =
```

The last line binds `files.chooser` to nothing, so no key opens the chooser,
and the help popup (`?`) marks its line **unbound**.

A key is written as the modifiers held with it, each followed by `+`, and
then the key itself. The modifiers are `ctrl` (or `control`), `alt`,
`shift` and `super`. On a Mac, `cmd` (or `command`) is the Command key and
`option` (or `opt`) the Option key; each is read on either system as the
same key as `super` and `alt`, so one file serves both. The key is one of:

- a character, written as your keyboard types it: `>` rather than
  `shift+.`, and `L` for `Shift` with `l`. `shift+l` means the same as `L`;
  `shift` with any other character is refused, since what `Shift` types
  there depends on the layout. A trailing `+` is the key `+`, so `ctrl++` is
  `Ctrl` with `+`.
- a digit, `0` to `9`, which is the key on the number row wherever your
  layout puts the character: `shift+2` is `Shift` with that key, whatever it
  types.
- one of `space`, `enter` (or `return`), `esc` (or `escape`), `tab`,
  `backspace`, `delete` (or `del`), `insert`, `home`, `end`, `pageup`,
  `pagedown`, `left`, `right`, `up`, `down`, and `f1` to `f12`. These names
  are read in any case; a single character is not.

`#` starts a comment, so to use the key that types it, write the number-row
key it is on: `shift+3` on most layouts.

A key has one job at a time. Binding a key to a name takes it away from
whichever name had it, so a key moved from one job to another does not also
keep the old one; where an earlier line of the file had given it to the
other name, `gamut` says so on the terminal.

The region's names — `region.move.*`, `region.grow.*` and `region.shrink.*`
— are the one exception. While a region is selected they are tried first,
and any other key does what it does without one. By default they are on the
arrows, which is why the arrows move a selected region rather than stepping
through the files. Bind `region.move.left` to `h` and Left steps to the
previous file under a region again, while `h` moves it; unbind
`region.move.left` and Left steps to the previous file under a region with
nothing moving it left.

Some keys are not in the table and cannot be rebound: `Esc` inside a popup
or dialog, `Enter` in the rename and export dialogs, and the arrows, `Page
Up`, `Page Down`, `Home`, `End` and `Enter` in the file chooser while you
type in it. The chooser's key only opens it; `Esc` closes it, and while it
is open its key types into it like any other. A long list of keys on one name
may wrap in the help popup.

### Gestures

Each thing the mouse can do on the picture or the minimap is a slot, named
`gesture.<surface>.<what is done>`, and holds one behavior:

```
gesture.image.middle.drag = pan
gesture.image.middle.hold = loupe
gesture.image.ctrl+wheel = exposure
gesture.image.left.drag = zoom-box
gesture.image.back.click = none
```

The surface is `image` or `minimap`. What is done is a button, held with any
modifiers, and how it is used: `left`, `middle`, `right`, `back` or
`forward`, then `.drag`, `.hold`, `.click` or `.double-click` —
`shift+left.drag`, `middle.click`, `left.double-click`. Or it is the wheel, turned with modifiers or with a button
other than the left held down: `wheel`, `ctrl+wheel`, `right+wheel`,
`ctrl+middle+wheel`. Or it is a scroll, written the same way — `scroll`,
`cmd+scroll`, `right+scroll` — which is two fingers on a trackpad or a wheel
that scrolls smoothly rather than in notches; a scroll with no slot of its
own does what the wheel with the same modifiers and button does. Or it is a
pinch on a trackpad, with modifiers: `pinch`,
`cmd+pinch`. The left button cannot be a hold, being a drag.

What each takes:

| Gesture | Behaviors |
| --- | --- |
| A drag | `pan`, `zoom-box`, `move-region`, or `none` |
| A hold | `loupe`, or `none` |
| The wheel, a scroll, or a pinch | `zoom`, `loupe-magnification`, `exposure`, `black-point`, `white-point`, `files`, `frames`, `pan`, or `none` |
| A click or a double-click | the name of a key, which it then does — `files.back`, `interface.grid`, `zoom.100` — or `none` |
| A drag or a click on the minimap | `center`, or `none` |

Each step of the wheel is a notch, and a trackpad's scroll adds up to a
notch before anything moves, except for `zoom` and `pan`, which follow a
trackpad smoothly; `pan` follows it both ways. Turning the wheel up raises
the exposure or the point it steps, and goes to the previous file or frame.
The first click of a double-click is a click as well, and does whatever
that button's click does. `move-region` moves a selected region when the drag starts inside it, and
elsewhere does whatever the same button does with nothing held.

A pinch outward is the wheel turned up. On a Mac the defaults add three
slots: `gesture.image.scroll` is `pan`, `gesture.image.cmd+scroll` is
`zoom`, and `gesture.image.pinch` is `zoom`; a mouse's notched wheel zooms,
as it does elsewhere.

A gesture is matched exactly: a slot for `left.drag` says nothing about
`ctrl+left.drag`, which does nothing unless it has a slot of its own. A
wheel with `Ctrl`, `Alt` or `Super` held does nothing by default, as those
belong to the window manager; on a Mac, only `Cmd` with the scroll does
something by default.

Some of what the left button does on the picture comes before its slot and
cannot be changed: a drag begun while a region is asked for draws it, one
from a handle pulls the handle, one with `zoom.fit`'s key held draws a box
to zoom to, and a click on a handle makes it the current one.

### Commands

A command is a line of the shell that `gamut` runs for you, with the file on
screen and what you have marked on it handed over in its environment. Each
one is a few lines of the file, all starting `command.` and its name:

```
command.redact.label = Redact region
command.redact.run = ~/bin/redact.sh
command.redact.needs = region
command.redact.input.style = radio black blur = black
command.redact.input.style.label = Style
keys.command.redact = ctrl+shift+r
```

Only `run` is required. A command appears at the foot of the file menu,
under the button before the file's name, and on a Mac in **File ▸
Commands**. The help popup lists it with its key. Bind a key to it with
`keys.command.<name>`, or a click with `gesture.image.<button>.click =
command.<name>`; a command has no key until you give it one. The name is
lower-case letters, digits, `_` and `-`.

| Line | What it does |
| --- | --- |
| `run` | The line the shell runs, as `sh -c` would. It runs in the folder of the file on screen, or your home folder with nothing open. A script's `$0` is `gamut` |
| `label` | What the menu, the help popup and the window's messages call it. The name, if left out |
| `needs` | What it cannot run without: `file` (the default), `region`, `pointer`, `location` (a photo that says where it was taken), `georeference` (a map placed on the ground), or `none`, which runs from the empty window too. Without it, the item is dead and its tooltip says why |
| `confirm` | `true` asks before running, even with nothing to ask |
| `shown` | `true` writes the picture as you see it, turned, exposed and toned, to a PNG for the command to read, in `GAMUT_SHOWN` |
| `input.<name>` | Something to ask before it runs. The window puts up a dialog with every input in the order the file first names them, and the answer reaches the command as `GAMUT_INPUT_<NAME>`, in capitals. The name is lower-case letters, digits and `_` |
| `input.<name>.label` | What the dialog calls that input. Its name, if left out |

An input is one of these, each with a default where the line gives none:

| Input | Written | Reaches the command as |
| --- | --- | --- |
| A line of text | `string`, then the default text | What was typed |
| A number | `number`, then a least and a most, then a default. All optional; the dialog refuses a number outside them | The number: `4.5` |
| A checkbox | `checkbox true` or `checkbox false` | `true` or `false` |
| A choice of words | `radio`, the words, then `=` and the default: `radio png jpg = jpg`. The first word, if no default | The word chosen |
| A slider | `slider`, a least, a most, a default and a step: `slider 0 1 0.5 0.05`. The default is the least, and the step 1 between whole numbers or a hundredth of the range otherwise | The number |

The dialog opens with what you last chose for that command in this
session. `Enter` runs it, `Esc` closes it.

A `run` line is read as every line of the file is: a `#` after a space
starts a comment there too, so a command that needs one belongs in a script
of its own, named by `run`.

#### What a command is told

Each of these is set only where there is something to say: no `GAMUT_LAT` on
a photo that does not say where it was taken, no `GAMUT_REGION_X` with no
region. Positions and sizes are in the file's own pixels, as the file holds
them, which is what another program reading the file counts in. A photo
whose EXIF orientation turns it, and a picture you have turned with `[` or
`]`, are counted as the file lies, not as you see them.

| Variable | What it holds |
| --- | --- |
| `GAMUT_FILE` | The file on screen, a full path |
| `GAMUT_DIR`, `GAMUT_NAME`, `GAMUT_STEM`, `GAMUT_EXT` | Its folder, its name, its name without the extension, and the extension without the dot |
| `GAMUT_INDEX`, `GAMUT_COUNT` | Which file of the list it is, counting from 1, and how many there are. `GAMUT_COUNT` is set with nothing open too |
| `GAMUT_FORMAT` | What read it, as the information panel's *Read by* line says |
| `GAMUT_WIDTH`, `GAMUT_HEIGHT` | Its size in the file's pixels |
| `GAMUT_ORIENTATION` | Its EXIF orientation, 1 to 8; 1 where it has none |
| `GAMUT_TURN` | How far you have turned it on screen: `0`, `90`, `180` or `270` degrees clockwise |
| `GAMUT_FRAME`, `GAMUT_FRAMES` | For an animation, the frame on screen counting from 1, and how many |
| `GAMUT_PAGE`, `GAMUT_PAGES` | For a file of pages, the same |
| `GAMUT_LAT`, `GAMUT_LNG` | Where the photo was taken, in degrees, negative south and west |
| `GAMUT_TAKEN` | When, with the camera's offset from UTC where the photo gives one: `2024-03-09T14:05:07-07:00` |
| `GAMUT_CAMERA` | The camera, as the information panel names it |
| `GAMUT_FOCAL_35MM` | The 35 mm-equivalent focal length |
| `GAMUT_FOV_H`, `GAMUT_FOV_V`, `GAMUT_FOV_D` | The field of view across, down and corner to corner, in degrees, worked out from that |
| `GAMUT_HEADING`, `GAMUT_HEADING_REF` | Which way the camera faced, in degrees, and whether from `true` or `magnetic` north |
| `GAMUT_POINTER_X`, `GAMUT_POINTER_Y` | The pixel under the pointer. A command chosen from the menu never has one, the pointer being on the menu: give it a key |
| `GAMUT_REGION_X`, `GAMUT_REGION_Y`, `GAMUT_REGION_WIDTH`, `GAMUT_REGION_HEIGHT` | The region, in the file's pixels |
| `GAMUT_SHOWN` | With `shown = true`: the picture as you see it, a PNG, taken away once the command is over |
| `GAMUT_SHOWN_WIDTH`, `GAMUT_SHOWN_HEIGHT` | Its size |
| `GAMUT_SHOWN_REGION_X`, `_Y`, `_WIDTH`, `_HEIGHT` | The region in it, as you see it |
| `GAMUT_RESULT` | The file to write what the window should do afterward, below |
| `GAMUT_VERSION` | Which version of `gamut` ran it |
| `GAMUT_INPUT_<NAME>` | What each input was given |

A map placed on the ground — a GeoTIFF, say — adds where its corners and the
rest are, in the map's own coordinates, and in latitude and longitude where
`gamut` can work them out:

| Variable | What it holds |
| --- | --- |
| `GAMUT_GEO_CRS` | Its coordinate system, `EPSG:` and the code, where it names one |
| `GAMUT_GEO_UNIT` | The unit its coordinates are in |
| `GAMUT_GEO_TRANSFORM` | Six numbers placing its pixels, in the order GDAL writes a geotransform: the left edge, a pixel's width, the row's skew, the top edge, the column's skew, a pixel's height |
| `GAMUT_GEO_UL_X`, `_Y`, `_LAT`, `_LNG` | Its upper left corner; `UR`, `LR` and `LL` the others |
| `GAMUT_GEO_CENTER_X`, `_Y`, `_LAT`, `_LNG` | Its middle |
| `GAMUT_GEO_POINTER_X`, `_Y`, `_LAT`, `_LNG` | The middle of the pixel under the pointer |
| `GAMUT_GEO_REGION_UL_X`, `_Y`, `_LAT`, `_LNG` | The region's upper left corner; `REGION_LR` its lower right |

#### What a command tells the window

To have the window show an image the command made, or say something,
write lines to the file `GAMUT_RESULT` names, written as this file's lines
are:

| Line | What the window does |
| --- | --- |
| `open = <path>` | Shows that image, joining it to the list beside the file on screen. A relative path is read from the command's folder. With several, the last |
| `message = <words>` | Says the words at the foot of the window |
| `error = <words>` | Says them as something that went wrong |

What a command prints is never read for these: a script printing what it
found in a file, such as its tags or the text in it, cannot make the window
do anything. What it prints goes to the information panel's **Output** tab,
which comes up when there is any, a line to a row; its errors in the warning
color. Click a line to copy it, or **Copy All** for every line. **Open log**
opens what it printed in your editor, as **Edit configuration file** does.
The last 64 runs' output is kept in `~/.cache/gamut/runs`, or
`~/Library/Caches/gamut/runs` on a Mac. Only the first megabyte of each
stream is kept.

A command that exits with an error is said in the window with the first
line of its error output, or with its own words where it wrote an `error`
line; an `open` it asked for is not done then. One that finishes without a
word says it is done. A file written beside the picture, or the picture
itself written again, appears without any `open` line, since the folder and
the file on screen are watched.

One command runs at a time; while one does, the others are dead. A command
that takes a moment is said at the foot of the window with a cross, which
stops it and everything it started. It has nothing to read from and no
terminal, so a command that waits for typing ends at once, and `sudo`
cannot ask for a password. Closing the window leaves a command running; it
ends with an error the next time it prints, so a long job should write to a
file.

The values reach the command through its environment, never spliced into
the line, so a camera name or a file name cannot run anything by itself.
What your command does with them is up to it: put each variable in double
quotes, and never `eval` one.

On a Mac started from the Dock or Finder, the shell does not read your
startup files, so a program installed by Homebrew is found only because
`gamut` adds `/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin` and
`~/.local/bin` to the end of the path. Elsewhere, name it by its full path.

#### Examples

These use programs you install yourself: ImageMagick's `magick`,
`tesseract`, `exiftool`, `curl` and `jq`.

Black out the region, in a new file beside the photo, and show it:

```
command.redact.label = Redact region
command.redact.needs = region
command.redact.run = out="$GAMUT_DIR/$GAMUT_STEM-redacted.$GAMUT_EXT"; magick "$GAMUT_FILE" -fill black -draw "rectangle $GAMUT_REGION_X,$GAMUT_REGION_Y $((GAMUT_REGION_X + GAMUT_REGION_WIDTH - 1)),$((GAMUT_REGION_Y + GAMUT_REGION_HEIGHT - 1))" "$out" && echo "open = $out" >> "$GAMUT_RESULT"
```

The coordinates are the file's own, so this blacks out the right place
whichever way the photo was held, and the new file keeps its orientation.

Blur it instead, as hard as the slider says:

```
command.blur.label = Blur region
command.blur.needs = region
command.blur.input.radius = slider 1 50 12
command.blur.input.radius.label = Radius
command.blur.run = out="$GAMUT_DIR/$GAMUT_STEM-blurred.$GAMUT_EXT"; magick "$GAMUT_FILE" -region "${GAMUT_REGION_WIDTH}x${GAMUT_REGION_HEIGHT}+${GAMUT_REGION_X}+${GAMUT_REGION_Y}" -blur "0x$GAMUT_INPUT_RADIUS" "$out" && echo "open = $out" >> "$GAMUT_RESULT"
```

Sign the picture in its lower right corner. `-auto-orient` first, since the
corner is the one you see:

```
command.sign.label = Watermark
command.sign.input.text = string © Your Name
command.sign.input.text.label = Text
command.sign.run = out="$GAMUT_DIR/$GAMUT_STEM-signed.jpg"; magick "$GAMUT_FILE" -auto-orient -gravity southeast -pointsize 48 -fill "rgba(255,255,255,0.6)" -annotate +32+32 "$GAMUT_INPUT_TEXT" "$out" && echo "open = $out" >> "$GAMUT_RESULT"
```

Save exactly what is on screen, exposure and false color included:

```
command.snapshot.label = Save as shown
command.snapshot.shown = true
command.snapshot.run = cp "$GAMUT_SHOWN" "$GAMUT_DIR/$GAMUT_STEM-shown.png" && echo "message = Saved $GAMUT_STEM-shown.png" >> "$GAMUT_RESULT"
```

Open where a photo was taken in OpenStreetMap (`open` in place of
`xdg-open` on a Mac):

```
command.osm.label = Open in OpenStreetMap
command.osm.needs = location
command.osm.run = xdg-open "https://www.openstreetmap.org/?mlat=$GAMUT_LAT&mlon=$GAMUT_LNG#map=17/$GAMUT_LAT/$GAMUT_LNG"
```

Say what place that is, by asking OpenStreetMap's search. This sends the
location over the network:

```
command.where.label = What place is this?
command.where.needs = location
command.where.run = curl -sf -A gamut "https://nominatim.openstreetmap.org/reverse?format=jsonv2&lat=$GAMUT_LAT&lon=$GAMUT_LNG" | jq -r '"message = " + .display_name' >> "$GAMUT_RESULT"
```

Read the text in the region into the Output tab:

```
command.ocr.label = Read text
command.ocr.needs = region
command.ocr.run = magick "$GAMUT_FILE" -crop "${GAMUT_REGION_WIDTH}x${GAMUT_REGION_HEIGHT}+${GAMUT_REGION_X}+${GAMUT_REGION_Y}" png:- | tesseract - -
```

List every GPS tag in the Output tab:

```
command.gps.label = GPS tags
command.gps.run = exiftool -a -G1 -gps:all "$GAMUT_FILE"
```

Copy the photo into a folder for the day, never over one already there:

```
command.pick.label = Copy to today's picks
command.pick.run = d="$HOME/Pictures/picks/$(date +%F)"; mkdir -p "$d" && cp -n "$GAMUT_FILE" "$d/" && echo "message = Copied to $d" >> "$GAMUT_RESULT"
```

Ask first before something that cannot be undone:

```
command.publish.label = Publish
command.publish.confirm = true
command.publish.run = rsync -a "$GAMUT_FILE" example.com:/srv/photos/
```

A longer command is better as a script of its own, named by `run`.

### Editing it from the window

`Ctrl+,` (`⌘,` on a Mac, or **Settings** in the menu bar), or **Edit
configuration file** at the foot of the help popup, opens the configuration
file in your editor. Where there is no file yet it writes one first, with every
setting commented out at its default.

On Linux the editor is the one `$VISUAL` names, or else `$EDITOR`, arguments
and all; with neither set, it is your desktop's default for text files. Where
the editor's desktop entry says it runs in a terminal, as `nvim`'s and
`vim`'s do, it opens in a new terminal window — the one `xdg-terminal-exec`
or `$TERMINAL` starts. An editor with no desktop entry is taken to run in a
terminal too. A graphical editor opens its own window.

On a Mac the file opens in the application that opens plain text, which you
choose in Finder: Get Info on any `.txt` file, then **Open with** and
**Change All**. `$EDITOR` is not read, since `gamut` started from the Dock
or Finder never sees it.

### While `gamut` is running

Saving the file puts the keys, the gestures, the commands, `open_map_link`,
`open_map_bounds_link` and `exiftool` in it in force at once, and the window says it was reloaded, or names the first line
it could not use. A command's dialog that is up closes; a command running
goes on.
Everything else in it is how the window opens: the panels stay as you have
them, and `browse_folder` and `max_files` wait for the next start.

## The state file

`~/.local/state/gamut/state` (under `$XDG_STATE_HOME` if you set it), or
`~/Library/Application Support/gamut/state` on a Mac, is where
`gamut` remembers settings you change by hand: how wide you dragged the file
list, what the list is sorted by and which way, how far the loupe
magnifies, how large you made the interface, whether a raw opens as the camera's JPEG,
whether the folder dialog's *Include subfolders* box was left ticked, and how the pixel
under the pointer is read out: its value, and for a map its place and how a
latitude is written. It is written when the window closes and read when the next window
opens. You never need to edit it, and deleting it resets them all to their
defaults.
