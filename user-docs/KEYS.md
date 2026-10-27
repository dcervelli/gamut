# Keys and mouse

Every control `gamut` has, at its default. Each key has a name — the last
column — that the [configuration file](SETTINGS.md#keys) can bind to other
keys, and each mouse gesture can be given something else to do there too;
the help popup (`?`) always shows what is bound now. A capital letter that
nothing is bound to does what its lower case does, so Caps Lock changes
nothing except where the two cases do different things: `a` and `A`, `s`
and `S`, `c` and `C`, `l` and `L`, `n` and `N`.

## On macOS

The tables below give each key as it is on Linux. On a Mac, the keys that
Linux holds with `Ctrl` or `Alt` are held with `⌘` instead, and a few of
the shortcuts every Mac program shares are added. Everything not listed here
is the same on both. Keys are written as a Mac's menus write them: `⌘`
Command, `⌥` Option, `⇧` Shift and `⌃` Control, run together onto the key.

| Key on a Mac | What it does | Name |
| --- | --- | --- |
| `1`, `0`, `⌘0` | Actual size | `zoom.100` |
| `+`, `=`, `⌘+`, `⌘=` | Zoom in one step | `zoom.in` |
| `-`, `_`, `⌘-` | Zoom out one step | `zoom.out` |
| `⌘`-Arrows | Pan to the far side of the image; with a region selected, grow it that way one pixel | `pan.edge.left` … `pan.edge.down`; `region.grow.left` … `region.grow.down` |
| `⇧⌘`-Arrows | Pan by one pixel; with a region selected, shrink it that way one pixel | `pan.pixel.left` … `pan.pixel.down`; `region.shrink.left` … `region.shrink.down` |
| `⌥⌘=`, `⌥⌘+` | Increase interface scale | `interface.scale.up` |
| `⌥⌘-`, `⌥⌘_` | Decrease interface scale | `interface.scale.down` |
| `⌥⌘0` | Reset interface scale to the monitor's own | `interface.scale.reset` |
| `?`, `/`, `⌘?` | Show the keys | `interface.help` |
| `⌘,` | Open the configuration file in your text editor | `interface.settings` |
| `q`, `⌘Q`, `⌘W` | Quit | `interface.quit` |
| `⌘P` | Choose a file from the list | `files.chooser` |
| `⌘[`, `⌘]` | Back and forward in image history | `files.back`, `files.forward` |
| `⌘O`, `⇧⌘O` | Open image files, or a folder, chosen in the file dialog | `files.open`, `files.open-folder` |
| `⌘⌫`, `⌦` | Move the file on screen to the Trash | `files.delete` |
| `⌘Z` | Undo | `files.undo` |
| `⌘E` | Export the picture as shown | `files.export` |
| `⇧⌘M` | Open a map of where the picture was taken | `files.map` |
| `⌃⇧⌘M` | Open a map of the area a georeferenced picture covers | `files.map.bounds` |
| `;`, `⌘L` | Turn the picture a quarter counterclockwise | `display.turn.left` |
| `'`, `⌘R` | Turn the picture a quarter clockwise | `display.turn.right` |
| `⌘C` | Copy the image, or the region, as displayed | `clipboard.image` |
| `⇧C`, `⌥⌘C` | Copy the file's path | `clipboard.path` |
| `⇧⌘C` | Copy the file as a URI | `clipboard.uri` |
| `⌘I` | Copy everything the info panel says | `clipboard.info` |
| `⌘.`, `⌘>` | Copy the pixel's value, or its coordinate | `clipboard.pixel`, `clipboard.coordinate` |
| `⌘V` | Paste an image | `clipboard.paste` |

`⌫` is the Delete key, which takes the file off the list as it does on Linux,
and `⌦` is the forward delete key: Fn-Delete on a laptop's keyboard. `F2`
renames on a Mac too, which on a laptop is Fn-`F2` unless the function keys
are set to act as function keys. `Return` plays an animation, as `Enter` does
on Linux.

The mouse differs as well. Two fingers on a trackpad, or a mouse that
scrolls smoothly, pan the picture, as they do in every Mac program that shows
one; a pinch zooms about the pointer, and so does the scroll with `⌘` held,
and a mouse's wheel that turns in notches. The
right button's hold is the loupe, as on Linux: a two-finger click, or
`⌃`-click, where there is no right button.

The help popup (`?`) and `gamut --help` show the Mac's keys spelled the same
way: `⇧⌘O`, `⌘`-Arrows, `⌦`, `Return`.

The menu bar holds what the buttons and keys do, with each item's key beside
it — the key bound now, so one changed in the configuration file shows there.
An item is grayed out where it would do nothing. Settings (`⌘,`) opens the
configuration file in the application that opens plain text — see
[Settings](SETTINGS.md#editing-it-from-the-window).

## Zoom and position

| Key | What it does | Name |
| --- | --- | --- |
| `1`, `0` | Actual size, one image pixel per screen pixel | `zoom.100` |
| Double-click | Actual size, or back to the whole image from actual size | `zoom.100.toggle` |
| `2`, `3`, `4`, `5` | 200%, 400%, 800%, 1600% | `zoom.200`, `zoom.400`, `zoom.800`, `zoom.1600` |
| `Shift`+`2`, `3`, `4` | 50%, 25%, 10% | `zoom.50`, `zoom.25`, `zoom.10` |
| `+`, `=` | Zoom in one step, a factor of 1.25 | `zoom.in` |
| `-`, `_` | Zoom out one step | `zoom.out` |
| `Space` | Cycle through the whole image, the window filled, and actual size; a fit dragged past the picture's edge goes back to the whole image first. With a region selected, fit the region, fill the window with it, then the image's three, in turn | `zoom.fit` |
| `Space`+Drag | Zoom to the box you drag out | `zoom.fit`, held |
| `p` | Cycle the filter used above 100%: nearest → bicubic | `zoom.filter` |
| `Shift`+Arrows | Pan by 64 pixels | `pan.left` … `pan.down` |
| `Ctrl`+`Shift`+Arrows | Pan by one pixel; with a region selected, shrink it that way one pixel, pulling its far side in | `pan.pixel.left` … `pan.pixel.down`; `region.shrink.left` … `region.shrink.down` |
| `Ctrl`+Arrows | Pan to the far side of the image; with a region selected, grow it that way one pixel | `pan.edge.left` … `pan.edge.down`; `region.grow.left` … `region.grow.down` |

Zoom runs from 2% to 6400%. The zooms below 100% are the ones above it with
`Shift` held, so each zoom is under the number it hangs off. The number row is
the one part of the keyboard read by where a key is rather than by what it
types, so `Shift`+`2` is 50% whatever character your layout puts there.

The two fits are the whole image, which leaves a margin on one side, and the
window filled, which runs the image off the ends of the other. Which axis
either one works out to is the image's shape against the window's. `Space`
runs through them and then actual size, and starts over at the whole image
from any zoom you chose by hand, actual size included, or from a fit you have
dragged past the picture's edge.

Every zoom works about the pointer while it is over the picture: the detail
under the pointer stays where it is, whether the zoom came from a key, the
zoom menu, a double-click or the wheel. With the pointer elsewhere — over a
panel, or beside the picture — the middle of the window stays put instead.
The two fits center the picture whatever the pointer is over. A zoom to the
zoom you are already at moves the detail under the pointer to the middle of
the window instead: `1` at actual size goes to what the pointer is on. A
double-click at actual size shows the whole image again, so a double-click
in and a double-click out is a quick look at the pixels. `zoom.100.toggle`
has no key by default; bind one to it to do the same from the keyboard.

Zooming leaves fit mode; panning does not, so `Space` and then `Shift`+Down scrolls
through a tall image filling the window.

To zoom to a part of the picture, hold `Space` and drag a box around it: the
box fills the window when you let go. The key answers when it comes up
rather than when it goes down, so a tap still cycles the fit, and holding
it while you drag does not move the picture first. `Esc` during the drag
drops the box.

The arrows on their own step through the files; held with a modifier they
pan. `Ctrl`+`Shift` with an arrow places the view to the pixel, which is what
lining two images up on the same detail takes; `Ctrl` with one runs to that
side of the image in a single press. None of them does anything when the whole image is already
on screen, since there is then nowhere to pan to.

## Moving through the files

| Key | What it does | Name |
| --- | --- | --- |
| Right, Down, `]`, `Page Down` | Next file | `files.next` |
| Left, Up, `[`, `Page Up` | Previous file | `files.previous` |
| `Home` | First file | `files.first` |
| `End` | Last file | `files.last` |
| `Ctrl+P` | Choose a file from the list: type to filter it, arrows to move, `Enter` to open, `Esc` to close | `files.chooser` |
| `Tab` | Show or hide the file list down the left of the picture | `files.list` |
| `Alt+Left`, `Alt+[`, `Alt+Page Up` | Back in image history | `files.back` |
| `Alt+Right`, `Alt+]`, `Alt+Page Down` | Forward in image history | `files.forward` |
| `Ctrl+O` | Open image files chosen in the desktop's file dialog | `files.open` |
| `Ctrl+Shift+O` | Open a folder chosen in the desktop's file dialog | `files.open-folder` |
| `F2` | Rename the file on screen | `files.rename` |
| `Del` | Move the file on screen to the trash, and show the next | `files.delete` |
| `⌫` (`Backspace`) | Take the file on screen off the list, and show the next | `files.remove` |
| `Ctrl+Z` | Undo the last rename, deletion or removal | `files.undo` |
| `Ctrl+E` | Export the picture as shown to a new JPG or PNG | `files.export` |
| `Ctrl+Shift+M` | Open map of image location in browser, for a picture that records its location | `files.map` |
| `Ctrl+Alt+Shift+M` | Open map of image bounds in browser, for a picture placed in latitude and longitude | `files.map.bounds` |

A file you have already looked at comes back exactly as you left it: the same
pan and zoom, the same turn, the same window and exposure, the same tone curve
and false color. Flipping between two images with `[` and `]` therefore compares them
rather than resetting them.

A file being opened for the first time keeps the pan and zoom when it is the
same size as the one on screen, so a directory of frames or exposures stays
comparable under the same pixels. One of the same shape at another size, such
as a map of the same ground at twice the detail, keeps the same spot in the
middle of the window at the same size on screen. One of a different shape is
a different image, and is fitted. Two georeferenced files in the same
coordinate system line up by place instead: stepping to one that covers any of
the ground on screen keeps that ground in the middle of the window at the same
scale, whatever their sizes or resolutions, and whatever view the file was
left in. Files that cannot be decoded are stepped over. The list is whatever
was named at startup, in name order to begin with and in whatever order the
file list's menus put it in after that; naming a directory puts the images in it
on the list, and keeps it up to date as images are added to that directory or
taken out of it.

`-r` or `--recursive` takes in the images in a directory's subfolders too,
however deep, and keeps every one of those folders up to date the same way;
a folder that appears inside it while the window is open is searched as well.
The window opens on the first image found and the rest join the list as they
are found, so a large tree or one on a network share can be looked at before
the search is over; while it goes on, the foot of the window counts what it
has found. Hidden folders — those whose names start with a dot, where other
programs keep their caches — are not searched, nor folders reached through a
link, and on a Mac nor a photo library or other package. The search stops
at 10,000 images and says so, which `max_files` in the configuration file
changes (see [settings](SETTINGS.md)): a tree that large is usually one
named by mistake, and every image on the list is one whose thumbnail is
made. A folder already being watched keeps adding what arrives in it after
that.

A single file opened by itself, as a file manager opens one, steps on
through the other images in its folder. The folder is not read until you
ask for more than the one file — `]` or `[`, `Ctrl+P` or `Tab` — so until
then the window shows just the picture, and the first step lands on the
file that comes next in the order the list is sorted by. A large folder, or
one on a network share, may take a moment to read; the window says so while
it does. `--alone` on the command line, or `browse_folder = false` in the
configuration file, keeps the list to the one file. With nothing else to
step to, `]` and `[` say so.

### The file list

`Tab`, or the button at the head of the top bar, puts the list down the
left of the picture: a strip of thumbnails, one row per file in the order
`]` and `[` walk, with the file's place in the list and its name above
each and the file on screen washed in the accent. A name too long for
the row loses its middle rather than its end, so that its extension and
any number before it still show. Resting the pointer on a row gives the
name in full, the folder it is in, the kind of file, its size in pixels
and on disk, and when it was last changed, on this machine's clock. A click on a row shows that file, and the strip scrolls to the file on screen as it
changes. The strip takes its width from the picture, which is fitted into
what is left; `Tab` again gives it back. Drag the strip's right edge to
make it wider or narrower: the thumbnails grow with it, from 128 pixels
across to 384. Each row takes its picture's shape — a landscape picture's
row shorter, a portrait's taller — up to half as tall as it is wide, or
half again as tall, beyond which a panorama or a tall screenshot is
fitted inside with room either side. A row is square until the file has
been looked at, which happens in the background shortly after the list
arrives. With a single file there is no
list to show: `Tab` reads its folder if it has not been read and puts the
list up once it is in, and otherwise does nothing you can see.

The menu at the head of the strip says how the list stands. It sorts the
files by name, by path, by kind of file, by when the file
was last changed, by when the picture was taken, by size on disk, or by
width, height or area in pixels,
and its last two rows run the sort ascending or descending; the button
wears which. Sorted by anything but the name, each thumbnail wears the
value it is sorted by across its foot — for the path, the folder, cut in
its middle where it is long. The list itself is put in
that order: the keys, the count in the bar and the chooser all walk what
the strip shows. A sort keeps the order of files it cannot tell apart, so
sorting by size and then by kind leaves each kind in size order, the way
sorting a spreadsheet twice does. The kind of a file, its size, its dates
and its dimensions are read from its header in the background, and a file whose
header has not been read yet sorts after those that have, and moves
into place once it is. A file that does not say when it was taken — a
screenshot, an export that dropped its metadata — sorts after every file
that does, whichever way the sort runs.

The capture date is the moment the picture was taken, so pictures from
cameras set to different time zones interleave in the order they were
really taken. Where the file records its camera's time zone, or a GPS fix
it can be worked out from, that moment is exact. Most cameras record
neither, and then the time is read as if it were in your computer's time
zone, which is right for pictures taken where you are and an hour or more
out for one camera set elsewhere against another. The thumbnail shows the
time as the camera's clock read it. The list opens in the order you last put it in,
or in name order the first time, whatever order the files were named in,
and the first file shown is the first in that order. Sorted by anything
but name or path, a long list is read for its order before its first file
opens, and the window says so while it is. The list stays in that order
as files arrive in a directory or leave it. Whatever the order does, the file on
screen stays the file on screen.

Beside the menus are two buttons that go back to the file shown before
this one and forward again, which `Alt+Left` and `Alt+Right`, `Alt+[` and
`Alt+]`, or `Alt+Page Up` and `Alt+Page Down` do from the keyboard: the files you have looked at,
in the order you looked at them, as a browser keeps the pages. Going back
and then somewhere new — a step, a pick from the chooser, a paste — cuts
off what lay ahead, so forward always leads to something reached from
here. A file that has left the list is passed over on the way back and
is there again once undo puts it back. The buttons are dead, and the keys
do nothing, with nowhere to go.

`Ctrl+P` opens a chooser over the picture: a field to type in, and under it
every file on the list that fits what you have typed, best fit first, each
with a thumbnail, its name, its title where the file carries one, what kind
of file it is, its place in the list and its size. The match is fuzzy —
`dsc17` finds `DSC_0017.JPG` — and runs over the directory as well as the
name when the list spans more than one, so `june/` narrows to that
directory, and over the title too, so `buzzard` finds `buteo-buteo.webp`
by what a cataloging program called it. The titles are read as the program
starts, ahead of the thumbnails, so on a long list they are all known
within a moment of opening the chooser. A query beginning with `:` asks by
place in the list instead: `:12` puts the twelfth file first, followed by
every file whose number has `12` in it, and a `-` counts from the end, so
`:-1` puts the last file first. The arrows move through the rows
without opening anything; `Enter`, or a click on a row, opens that file and
closes the chooser; `Esc`, or a click outside it, closes it.
With a single file on the list there is nothing to choose: `Ctrl+P` reads
its folder if it has not been read and opens once it is in, and otherwise
does nothing. While it is open, keys go into the field rather than to the picture. The
file on screen is marked in the list, and the cursor starts on it, so
`Down` and `Enter` is the next file.

The thumbnails are the desktop's own, kept under `~/.cache/thumbnails` where
your file manager keeps them: one it has already made is shown without
decoding the file, and one made here is one it will show. They are made in
the background from the moment the program starts, at low priority, so a
long list fills in over time rather than holding anything up, and a file
whose thumbnail has not yet been made shows an empty slot until it has. A
slot with a struck-through picture in it has no thumbnail coming: the file
could not be read, or is too large to thumbnail in the background — over
about 350 megapixels for an 8-bit RGB PNG, 67 for most other formats — in
which case opening it makes one. The row's tooltip says which. A
Mac has no cache that file managers share, so there they are `gamut`'s own,
under `~/Library/Caches/com.dcervelli.gamut/thumbnails`.

### Opening something else

`Ctrl+O` puts up your desktop's own file dialog, narrowed to the image
formats this program reads, with an **All files** filter for a file named
without its extension. Choose one file or several: what you choose joins
the end of the list, exactly as if it had been named on the command line
after everything else, and the first of it in the list's order is shown. `Ctrl+Shift+O` puts up the same dialog for a
folder, which stands for the images inside it as a directory on the command
line does — kept up to date as images arrive in it or leave. Its
**Include subfolders** box takes in every image under the folder, as
`--recursive` does, and is left as you last left it. Two keys rather
than one because a desktop's dialog picks files or it picks a folder, never
both at once. Cancel the dialog and nothing changes. The dialog starts in
the folder of the picture on screen, or of the last one shown.

Opening adds to the list rather than replacing it: `]` and `[` and the
chooser walk everything named so far, in whatever order the file list has
put it in, and a file already on the list is not added twice — choosing it
again simply shows it, as you left it. A file taken off the list with
`Backspace` comes back onto it this way. The picture on screen stays until the first new
file has arrived, as it does when stepping.

Started with no path at all — `gamut` on its own, or from the desktop's
menu — the window opens empty, with three buttons in the middle of it: the
two dialogs, and a paste of the picture on the clipboard, which is dead
until there is one. The first picture to arrive in an empty window sizes
it as the window would have opened on that picture, unless `--size` chose
the size. The window comes back to those buttons when nothing it was
handed could be opened, and says why at its foot, and when the last file
on the list is deleted or taken off it. Then, while the folder the last
picture came from still holds images, a fourth button above the others,
**Open all in** and the folder's name, opens every image in it, as
choosing that folder in the dialog would. While the dialog is up, the keys
and the buttons that put it up do nothing more.

The dialog is the desktop's, asked for through its portal, so it looks and
behaves as the dialog of every other program on your desk does. A desktop
without one — no `xdg-desktop-portal` and a backend for it running — cannot
put one up, and the window says so instead. On a Mac it is the standard open
dialog.

### Renaming and deleting

The button before the file's name in the top bar opens a menu of the file
itself: copy its name, copy its path, copy it as a `file:` URI, rename it,
take it off the list while there is more than one file, delete it, export
it. The keys above do the same without the menu.

`Backspace` takes the file on screen off the list and shows the next, or
the previous one from the end of the list, leaving the file exactly as it
is on disk: a way of culling what you are walking through without
throwing anything away. The file stays off the list for the session,
however often its directory is read again; `Ctrl+Z` puts it back where it
stood and shows it, and opening it by name again — from the dialog, or
the command line — does too. It stays on screen until its neighbor is up,
as a deleted file does, and the last file on the list leaves at once and
empties the window.

`Delete` moves the file to your desktop's trash — the same one your file
manager shows, so it is there to restore or empty from there whether or not
this window is still open — and shows the next file, or the previous one
from the end of the list. The file leaves the list once its neighbor is up.
With only one file there is nowhere to go: the file leaves at once, and
the window is empty — the three buttons that give it something — until
you open something or undo, which puts the file back and shows it. A
neighbor that will not open leaves the deleted file on screen, its name
struck through in red in the bar, until you undo or step away. A file on another disk
or a removable drive goes to the trash on that drive; where that cannot be
made, it is copied into your home trash instead, which takes as long as the
copy takes. On a Mac the file goes to Finder's Trash. A file restored there
with Put Back is no longer one undo can bring back, and if `gamut` is not
allowed to look inside the Trash, undo says so and names Put Back instead:
a program started from Terminal may need Full Disk Access for that.

`F2` opens a dialog with the name in a field, the part before the extension
selected so that typing replaces it and keeps the extension. What is wrong
with the name is said under the field as you type — a name already taken, a
slash, since the dialog renames and does not move — and the field's outline
turns red while the name will not do; `Enter` and OK then do nothing. A
change of extension is allowed, and noted in yellow: the file's contents do
not change with its name. `Enter` or OK renames, `Esc`, Cancel or a click
outside puts the dialog away. Everything that knew the file by its name
follows it: the bar, the title, the list, and the pan and exposure it will
come back to.

`Ctrl+Z` undoes the last of these, and then the one before, back through the
session: a deleted file comes out of the trash and back into the list where
it was, a renamed file gets its old name back, a removed file goes back
onto the list where it stood, and whichever it is, it is the file on
screen afterwards. Only what touched the disk or the list is on that
stack — zoom, exposure and the rest are put back by hand, or by `z`. An undo that cannot
be done says why: the trash was emptied in the meantime, or something else
now has the name. Nothing is remembered across a restart; after that the
trash itself is the record.

### Exporting

`Ctrl+E` exports the picture as you see it to a new file beside the one on
screen: turned as it is turned, cropped to the region if one is up, and with
the window, exposure, curve and false color written into the pixels. It is
what `Ctrl+C` copies, kept as a file. The file on screen is never changed.

The dialog offers a name — the file's own with `-edited` after it — and a
choice of PNG or JPG, starting on the format the file is already in where it
is one of the two. Typing an extension picks its format, and picking a
format changes the extension. The slider under JPG sets its quality, from 1
to 100; it starts at 90. A name already in the folder is refused, and Export
does nothing until it is changed: nothing is ever written over.

Under the formats, three boxes set the size the picture is written at: a
percentage, a width and a height. They say one thing between them — the
aspect is locked — so typing in any one fills in the other two; `Tab`
moves between them. The percentage is of the region when one is up, since
that is what is written. A side runs from 1 to 32768 pixels, and a box
that asks for more, or holds something that is not a number, is outlined
and holds Export until it is put right. A picture made smaller averages
the pixels each new one covers, as the window does below 100%; one made
larger is enlarged bicubic whatever the window's own filter, and the
dialog says so when the window is showing nearest.

Under the formats, the dialog warns of what the new file loses that the
screen does not show:

- A file with more than 8 bits a channel, or light above white, is written
  as 8-bit SDR, as the screen shows it without HDR.
- JPG drops transparency.
- Only the frame or page on screen is written, not the whole animation or
  every page. A playing animation stops while the dialog is up, so the frame
  written is the one on screen when you opened it, and plays again once the
  dialog goes.
- The file's metadata, such as EXIF and XMP, is not carried over, so the
  date taken, the camera and the location are not in the new file.
- With a region up, only the region is written, at its size — or at the
  size the boxes ask for, the percentage counted from the region's.
- A picture enlarged is enlarged bicubic, where the window shows nearest.

`Enter` or Export writes the file, `Esc`, Cancel or a click outside puts the
dialog away. The new file joins the list after the file on screen and is
shown, upright and at its own settings, since what was done is now in its
pixels. One export is written at a time: while a large one is still being
written, the dialog opens but Export waits, and says so. Stepping back finds the original as you left it. An export is not on
the undo stack; delete the new file as you would any other.

## Playing an animation

| Key | What it does | Name |
| --- | --- | --- |
| `Enter` | Play or pause an animation | `playback.play` |
| `n` | Next frame of an animation, or page of a file that holds several pictures | `playback.next` |
| `Shift+N` | Previous frame, or page | `playback.previous` |

An animated file plays as it opens, at the speed the file says, and loops as
many times as it says; `--paused` opens it stopped on the first frame. A
step with `n` or `N` pauses it on the frame it lands on, and goes round the
ends, so `N` on the first frame is the last. Once an animation that plays a
fixed number of times has finished, `Enter` plays it again from the start.

The same two keys step through the pictures of a file that is not an
animation — the icons in an ICO, the pages of a TIFF — one picture at a time,
with nothing to play. The bar under the picture shows which frame or page is
up, and for an animation how far into it that is; dragging along its timeline
goes to any frame.

A file left part way through comes back where it was left, playing if it was
playing, when you step off it and back.

## Copying and pasting

| Key | What it does | Name |
| --- | --- | --- |
| `c` | Copy the name of the file on screen, without its path | `clipboard.name` |
| `Shift+C` | Copy the absolute path of the file on screen | `clipboard.path` |
| `Ctrl+Shift+C` | Copy the file on screen as a URI | `clipboard.uri` |
| `Ctrl+C` | Copy the image itself, as you are seeing it — or the region, while one is selected | `clipboard.image` |
| `Ctrl+I` | Copy everything the file information says about the file | `clipboard.info` |
| `Ctrl+.` | Copy the value of the pixel under the pointer | `clipboard.pixel` |
| `Ctrl+>` (`Ctrl+Shift+.`) | Copy the coordinate of the pixel under the pointer | `clipboard.coordinate` |
| `Ctrl+V` | Paste an image, saved among your pictures and shown | `clipboard.paste` |

Every copy says so: a short message appears at the foot of the window naming
what was taken, and goes on its own after a couple of seconds. Copying changes
nothing you can see in the image, so without it there is no way to tell a
copy that worked from a key that was not read. Click the cross on the message
to take it off sooner, or press `Esc` — which closes whatever is up, a popup
first, then the panels if they are hidden, then a message, and quits only when
there is nothing left to close.
`q` quits whether or not a message is showing. A message in the theme's yellow
means what you asked for could not be done and nothing is wrong — the pointer
was not over a pixel, say; one in its red means the copy failed, and the
terminal has the reason in full.

`c` copies the file's own name and nothing else — `sunset.tif`, not the
directory it sits in — which is what you want when you are naming the file to
someone rather than pointing a program at it.

`Shift+C` copies the path in full, from the root down, whichever way the file
was named when the program was started — a relative name is of no use in
another window, which is where a copied path is going.

`Ctrl+Shift+C` copies the same file as a URI instead: `file://` and the path,
with spaces and punctuation escaped. This is what a file manager, a browser or
another program's open dialog asks for when it wants the file itself rather
than words about it, so pasting into one of those opens the image rather
than typing its name. Pasting into a text field still yields the URI.

`Ctrl+C` copies the image rather than a name for it, ready to paste into an
editor, a document or a chat window. What travels is what you are looking at:
the window, the exposure, the tone curve and any false color are all applied,
so a raw scan you have brought up out of the shadows arrives brought up. It is
the image at its own size, not a photograph of the window — the zoom, the pan
and the panels are how you are looking at it and none of them are copied.

A single-channel image stays single-channel, so a grayscale scan does not
arrive as three copies of itself, and false color is the one thing that
widens it. Transparency comes along only where the file had some; an opaque
image arrives opaque rather than carrying an empty channel. Everything
arrives 8-bit, which is the depth the screen was showing it at.

`Ctrl+I` copies everything the file information panel says, one line per
field: the section it stands under, its name, and what it says, separated by
commas so that it opens as a table in a spreadsheet and reads as plain lines
anywhere else. It works whether or not the panel is showing — what it says is
a fact about the file, and asking for it should not mean first arranging to
look at it. Single fields and single sections can be copied from the panel
itself; see below.

`Ctrl+.` copies the value the bottom left corner is reading out, exactly as it
is written there — so with the readout in hex, as it starts, `Ctrl+.` puts
`E78040` on the clipboard, and after switching it to decimal with `.` it puts
`231 128 64` there instead. `Ctrl+Shift+.` copies the pixel's coordinate
rather than its value, as `x,y` with nothing around it: `1919,1079`, ready to
paste into a command line or a spreadsheet. In a georeferenced file whose
readout is showing where the pixel is on the ground, it copies that instead,
in the same form: `2600000,1200000`, or `46.951083,7.438632` for a latitude and
longitude, which a map's search box takes as it is. Both work on the pixel the pointer
is over at the moment you press them, and say so on the terminal when the
pointer is not over one.

The image is prepared in the background, so the window keeps answering while
a large one is being got ready — a photograph of some tens of megapixels takes
a fraction of a second, and only then is there anything to paste. Copying
something else in the meantime wins: whichever copy you asked for last is the
one you get, not whichever happened to finish last.

Any of the copies outlives the window: closing `gamut` leaves it on
the clipboard, and it stays there until something else copies over it. Quitting
straight after copying is safe — an image still being prepared is finished
before the window goes.

`Ctrl+V` goes the other way: it takes the image on the clipboard, writes it
into your pictures directory, and shows it. The file is a real one and it
stays — a screenshot, or an image copied out of a browser or an editor, has
no file behind it, and one that vanished when you closed the window would be
no use to come back to. It is named for the moment you pasted it, in the same
form a screenshot is named in: `pasted_2026-09-04_11-40-32.png`. Where it
goes is wherever your desktop keeps pictures — `~/Pictures` unless you have
told it otherwise — and the directory is made if it is not there yet.

There is a button for it too, in the left strip under the region button. It
is there only while the clipboard is holding an image `gamut` can show, so
what it says is not only that pasting is possible but that there is something
to paste — it appears when you copy an image in another window and goes when
something else is copied over it. `gamut` looks at the clipboard four times a
second to keep it up to date, and stops looking while the interface is hidden
with `` ` ``.

The pasted file joins the list beside the one you were looking at, so `[`
goes back to where you were and `]` carries on. It stays on the list for as
long as the window is open, even when what you opened was a directory that
knows nothing about it.

`--paste` on the command line does the same before the window opens: the
image on the clipboard is written to your pictures directory and shown
first, ahead of any path you named, so `gamut --paste` alone opens what you
just copied. With nothing on the clipboard that `gamut` can show, it says so
on the terminal and opens the paths instead — or, with no paths, does not
open at all.

What is pasted is whatever the image was copied as, saved as it stands:
nothing is re-encoded, so a JPEG arrives a JPEG. `Ctrl+V` does nothing if the
clipboard holds words rather than an image, or holds it in a format `gamut`
cannot read — the window says "No image on the clipboard." and stays as it
was.
Copying a *file* in a file manager copies its name and not its contents, and
that is not a paste; open it as an argument instead.

## The display

| Key | What it does | Name |
| --- | --- | --- |
| `d` | Exposure down a quarter stop | `display.exposure.down` |
| `f` | Exposure up a quarter stop | `display.exposure.up` |
| `a` | Black point down | `display.black.down` |
| `s` | Black point up | `display.black.up` |
| `Shift+A` | White point down | `display.white.down` |
| `Shift+S` | White point up | `display.white.up` |
| `\|` | Toggle photo/data mode | `display.reading` |
| `e` | Cycle the window rule: as stored (0–1) → full range → trimmed (the central 99.8%) | `display.window` |
| `t` | Toggle the curve on the highlights: clip, or roll off | `display.tone-map` |
| `w` | Mark clipped pixels, or stop: red where a channel has reached white, blue where one has reached black | `display.marks` |
| `o` | Turn the room above white off and on, where the monitor is in HDR mode | `display.hdr` |
| `v` | Show a raw as the camera's JPEG of it, or as the picture developed from the sensor | `display.camera-jpeg` |
| `Shift+D` | Show a picture's depth map in its place, or the picture again | `display.depth` |
| `Shift+G` | Show a picture's gain map in its place, or the picture again | `display.gain-map` |
| `Shift+M` | Show a raw's mosaic in its place, or the developed picture again | `display.mosaic` |
| `Shift+P` | Show a raw as its camera profile renders it, or as developed | `display.profile` |
| `r` | Cycle false color: gray → viridis → magma → turbo | `display.colormap` |
| `z` | Reset every display setting | `display.reset` |
| `;` | Turn the picture a quarter counterclockwise | `display.turn.left` |
| `'` | Turn the picture a quarter clockwise | `display.turn.right` |

The two turn buttons near the right of the bottom bar do the same.
A turn changes how the picture is shown, not the file: it is kept with the
file while the window is open, like the exposure, and carries through an
animation's frames and a file's pages. A region turns with the picture, and
the pixel readout counts from the top left of the picture as you see it.
`z` leaves the turn alone. To keep a turn, export the picture with `Ctrl+E`.

A camera raw file holds two pictures: the one `gamut` develops from what
the sensor counted, and the JPEG the camera rendered and wrote into the file
beside it, with the camera's own curve, color and adjustments. While a raw
that carries a JPEG is on screen, a button with a camera on it, between the
turn buttons and `HDR`, names the picture you are looking at, **RAW** or
**JPEG**, and a click on it or `v` shows the other one. The choice
holds for every raw you open after it, and for the next time `gamut` starts.
Where you were looking stays where it was, at the same size, so the two can
be compared by flicking between them. The display settings start over, since
an exposure set for the developed picture means nothing to the JPEG. Export
with `Ctrl+E` writes whichever picture is up, which is the quick way to a
file that looks as the camera showed it.

Some cameras write a JPEG the full size of the frame; others write a smaller
one, around two megapixels, and the top bar says how large the picture on
screen is. A raw with no JPEG in it has no button, and shows its developed
picture while the camera's JPEG is chosen, saying so as it opens. A raw that
cannot be developed has no button either, and shows its JPEG while the
developed picture is chosen, saying so as it opens.

A picture that carries a depth map — a phone's portrait, say — has a
button with three axes on it, the third dashed, between that one and `HDR`.
A click on it or `D` shows the
depth map in the picture's place and lights the button; another shows the
picture again. The map covers exactly what the picture covered, so the same
thing stays under the pointer and the two can be flicked between; nothing is
read from the file again either way.

While it is up, the map is the image: the top bar gives its size — usually
far smaller than the photograph's — and the zoom, the pixel readout, the
histogram, the display keys, a copy with `Ctrl+C` and an export all work on
it, exactly as the camera's JPEG of a raw is the image while that is up. It
opens in gray, stretched from the lowest value it holds to the highest: in
an iPhone's portrait the near things come out light, and in a map stored the
other way round they come out dark. Its exposure, window and false color are
its own, and the picture's are waiting as you left them when you go back. A
region you had marked out goes, since it was marked in the other image's
pixels. The toggle stays on as you step through other pictures that carry a
map, and a picture without one is shown as itself.

A picture that carries a gain map — an HDR photograph from a recent phone —
has a button with a sun on it, just before `HDR`. A click on it or `G`
shows the gain map in the picture's place in the same way, and another
shows the picture again. The map is drawn in stops: black where it leaves
the photograph as it is, white where it brightens it most, stretched between
the two. The pixel readout gives the stops at the pixel under the pointer,
whatever your display's room, since it is the map the file holds and not how
much of it is being applied. A map with a channel for each color is drawn in
color. The gain map's button and the depth map's take each other's place:
with both on a picture, one map is up at a time.

A raw from a camera with a color filter over its sensor — nearly every
camera — has a button with a four-square grid on it, before the depth map's.
A click on it or `M` shows the mosaic in the picture's place: what the sensor
counted, one value to a photosite, each in the color of the filter over it,
before any of it was blended into full-color pixels or balanced. Zoomed in,
the filter's repeating pattern shows as single red, green and blue pixels;
zoomed out, the picture looks green, since a filter pattern has
twice as many green photosites as red or blue and nothing has been balanced
or brightened. Each photosite runs from the sensor's black to its white, and
the display keys and the pixel readout work on it as on the picture. The
histogram's *Image* section still plots the sensor's counts, with the
photosites around the pointer at its foot. Another click shows the developed picture again,
and the toggle stays on as you step through other raws. An iPhone's ProRAW
has no button, since it holds full-color pixels rather than a mosaic.

Some raws say how they should look: an iPhone's ProRAW carries an exposure,
a brightening of the shadows that varies across the picture, and a tone
curve, which together are how the iPhone rendered it. `gamut` shows such a
raw that way, and `P`, or the camera button at the end of the *Profile*
heading in the histogram panel, shows the picture as developed instead,
without them: much darker for most iPhone pictures. The choice holds for
every raw, and for the next time `gamut` starts. Nothing is read again, so
the switch is immediate, and the view and the display settings stay as they
are. `G` shows how far the profile brightens each pixel in the picture's
place, in stops, as it shows a gain map, and its button, just before `HDR`,
says the map is calculated from the camera's profile.

Under the *Profile* plot are the rendering's stages, read upward in the
order they are applied: the tone curve; the gain table, how far it lifts
a pixel by how bright it is, with the spread across the picture as a band
and the pixel under the pointer as a line; the exposure on its own row;
and at the foot the pixel in *ProPhoto*, the space the profile works in,
which a matrix carries it into from the developed picture. Pointing at the
picture follows the pixel up the stages a color at a time, every number
in stops below white and each in its color: the three in ProPhoto, the
three after the exposure, the three after the gain in the table's caption,
and the three the curve makes of them in the curve's. Each stage's numbers
are the next one's inputs, and the *Total lift* row under the plot at the
top of the section is how far the profile lifted each color of the pixel,
which is what that plot counts. The table is read once a
pixel, at a blend of its colors — the dot on its curve, with the gain it
reads there — and the one gain multiplies all three, shown as ticks on the
axis. The curve is applied to the brightest and dimmest colors, the dots
on it, and the middle one keeps its place between them, the hollow dot.
Resting on a stage says what it is.

The window is the one place the case of a key matters: `a` and `s` move
the value that comes out black, `A` and `S` the value that comes out white.
Each press moves its end by a twentieth of the window's width as the
histogram draws it, and no further than the histogram's axis goes.

Moving either end by hand takes the window out of whichever rule it was
on. In data mode, `e` cycles back into the rules and the *As
stored* button on the histogram panel puts the window back at 0 to 1; in
photo mode the rules are not offered, and the reset button puts the window back
with everything else. The exposure is a push on top of whatever the window
is, and neither handle touches it. Exposure stops at ±16 stops.

The histogram panel (`h`) is where all of this is seen and set. The band of
gray under the plot is what the display makes of each value along the axis,
and the two handles on it are the window: the left one stands at the value
that comes out black, the right one at the value that comes out white, and
either can be dragged to a new value — `a`/`s` and `A`/`S` step the same
two handles. Dragging the band between them slides the window along the
plot, as far as the plot goes: once the window is narrower than the axis —
after a stop or two of exposure on a graded file, or a trimmed window on
linear data — the band is which part of the range you are looking at. The
handles are the window on every file, and the exposure is a separate thing
on top of it: bringing the white handle in and raising the exposure make
the same picture, but the window is found again when a file changes on disk
or a *Window* rule is pressed, and the exposure survives that. A row of stops under
the Output and Display plots counts from white — 0 is what comes out white,
`-2` is two stops under it, and on an HDR screen `+1` is a stop over — and
the one under the gain map's plot counts how far the map lifts the picture.
A raw that says how it should look has a *Profile* section instead, whose
plot counts how far that rendering lifts each pixel. The rows of stops
under the tone curve and the gain table count the brightness going in,
from white: the curve's toe, where it takes the deepest shadows to black,
differs from one picture to the next.

For a camera raw, the *Image* section at the foot is the sensor's own
counts: one plot with a line for each color under the filter, from nothing
to the sensor's white level, with the black level marked on the axis, so
the room to the right of a line is how far the sensor was from saturating.
Pointing at the picture writes the one count under the pointer in its
color, since a photosite holds one color; a raw that stores three counts
a pixel writes all three, with a `~` before them where the development
clipped a channel and they can only be worked back approximately. Above
it, the *Develop* section is the picture as it was developed from those
counts, plotted in stops below white with its readout in the same stops,
with the demosaic, the balance each color was multiplied by in
stops — *applied in the file* where the camera balanced the counts before
writing them — the light that balance is for as a color temperature, and
the share of each channel standing at white: a channel against the right
edge there and short of it in the *Image* plot below was put there by the
balance, not by the sensor. Resting the pointer on the demosaic's name
says what it does, on the clipped row what its shares are of, and on the
*Profile* section's *Pixel* row what its numbers are. At the foot of the
*Image* section, for a mosaic, are the photosites around the pointer, a
window of five by five counts in their colors, which is what the demosaic
read to make the pixel. The eye at the end of their caption shows the
mosaic in the picture's place, as `M` does; while it is up, the *Develop*
section is left out.
A raw's *Display* plot is spaced in stops too, as a file of linear floats
is, since both are linear light, and its readout is stops below white: a
dark frame spreads across the plot instead of piling against its left
edge.
Pointing at a plot names the value under
the pointer at that stage; pointing at the picture marks the pixel on every
plot and writes its value at each stage, from the file at the foot of the
panel to what the screen shows at the top.

The pair of buttons at the head of the panel, *Photo* and *Data*, says which
mode the file is read in, and switches it; `|` toggles between the two. In
photo mode the file is shown as it was made to look, with 1.0 as white; in
data mode it is numbers windowed to what they hold, a single channel spread
along a ramp with no exposure to push. The program
guesses from the file, and guesses wrong on some: a 16-bit grayscale scan
saved without its profile opens as data, auto-leveled, and pressing *Photo*
reads it again as the photograph it is; an elevation model saved as an
ordinary 8-bit image opens as a picture, and *Data* reads it as the
measurement it is. The choice stays with the file for the rest of the
session, so stepping away and back finds it in the same mode, and the
same pair is under the Image heading of the information panel, beside
*Mode*. While the gain map or the depth map is shown in the picture's place
the pair is off: a map is always in data mode.

`w`, or the warning sign in the row of buttons at the top of the panel,
paints the clipped
pixels on the picture itself: red where any channel has reached white,
blue where any has reached black. One channel is enough, because one is
enough to lose the color: a sun whose red has burned out is painted red
though its green never reached white, and the pink it shows is not what
the scene held. The marks stay on until pressed off again, from one file
to the next, and no reset of the display touches them. A color
from a wide-gamut file — Display P3, Adobe RGB, a raw — that an ordinary
monitor cannot show is painted too, since the screen loses it the same
way. Neither marks white where nothing is being clipped: on
an HDR surface with no curve, or under a curve, the highlights are shown
or rolled off rather than lost.

Under the band of a photo, the exposure: a slider over six stops each way,
its reading at the end of the row. It snaps to the quarter stops `d` and
`f` take, and a press anywhere along it puts the exposure there. The keys
go on past the slider's ends, to ±16 stops, where the handle stands hollow
at the end and the reading says where it is. Under it, the *Curve* row —
*Clip* or *Roll off* — that `t` toggles, since a stop or two of exposure
puts the top of any picture above white.

In data mode a file has neither: its numbers need not be light, so there
is no exposure to push — the white handle does the same job in the file's
own units — and a curve would bend the values you are reading off. `d`,
`f` and `t` do nothing on it, and values past the top of the window are
clipped at white, on an HDR monitor too. Instead it has a row of windows,
which the `e` key cycles, named for what they do: *As stored* shows the
values as they are, 0 to 1; *Full range* stretches everything the file
holds to black and white; and *Trimmed* leaves the outermost 0.2% out
before stretching, which is what data — sensor counts, a float TIFF, an
elevation model — opens with. A photo has no such row: it opens as stored,
stretching it would only make it look wrong, and the handles are the way
into its shadows and highlights. To push the exposure of a file in data
mode, switch it to photo mode. The rows are the same for every file in
photo mode, and the same for every file in data mode, so the panel changes height only between the two.

The curve is added to bring values brighter than white back down into a
surface that cannot show them. *Clip* is not a second curve but the absence
of one: on an ordinary (SDR) surface the highlights are clipped at white,
and on an HDR surface they are shown at the brightness they were graded to.
*Roll off* brings them back under white with a curve that leaves everything
below its shoulder as it was, so the rest of the picture does not move to
make room. Every image starts clipped, on either surface: the bottom bar
says **clipped** when highlights are being thrown away, and `t` is the
answer. The curve stays as you set it when the room is switched with `o`
or the window lands on a different kind of monitor, and each file keeps
its own. `--tone-map none` and `--tone-map neutral` are the two choices at
start-up.

The bottom bar says **rolled off** while the curve is on, and **clipped**
when there is none, the surface is SDR and highlights are being thrown away
— so a photograph pushed a stop up says so rather than going flat in
silence. The
`HDR` button at the end of the bar is the switch for that room, lit while the
image is going out with it. On Wayland the compositor says which monitors
are in HDR mode, and on a Mac the display does; gamut follows: the window gets an HDR surface on a
monitor in HDR mode and an SDR one otherwise, and never asks the compositor
to switch a monitor over — a request some compositors answer by blanking
every display. On a monitor in HDR mode the button turns the room off and on
without touching the surface. On one in SDR mode, or where no HDR color
space is offered, it is drawn dead: neither the button nor `o` does anything,
and resting the pointer on it says which of the two it is. `--output hdr` asks for the HDR surface regardless, for anyone who
wants the compositor's own switch, and `--output sdr` stays on the SDR one
whatever the monitor is. Where nothing says what the monitor is, `o` moves
the surface itself.

False color applies to single-channel images only, and `r` does nothing on a
color one. While a colormap is active, tone mapping is suspended — a curve on
top of a colormap would distort the values you are reading off it — so the
histogram's row of curves goes dim, `t` does nothing, and both come back
with the gray ramp.

Everything on this page shows up in one line at the right of the bottom bar,
in front of the buttons there: the window by name, the exposure in the quarter
stops it is stepped in (`+¼ EV`), the false color, and what is becoming of
the highlights. Only what is in force is named, so an image you have not
touched leaves that end of the bar empty, and anything the window is too
narrow for is dropped from the end rather than cut off. Resting the pointer on
those words says the same thing in sentences, a line to each, with the
window's own bounds written out; clicking them opens the histogram, which is
where all of it is set.

## The interface

| Key | What it does | Name |
| --- | --- | --- |
| `h` | Show or hide the histogram | `interface.histogram` |
| `i` | Show the file information on its *Curated* tab, or hide it | `interface.info` |
| `I` | Show the file information on its *Raw Data* tab, or hide it | `interface.raw-data` |
| `m` | Show or hide the minimap | `interface.minimap` |
| `g` | Show or hide the grid over the image | `interface.grid` |
| `b` | Show or hide the horizon grid, for a picture that records which way the camera faced or its compass heading | `interface.horizon-grid` |
| `B` | Cycle the horizon grid's spacing: 5°, 10°, 15° or 30° | `interface.horizon-step` |
| `l` | Show or hide the loupe | `interface.loupe` |
| `Shift+L` | Cycle the loupe's magnification: 2, 4, 8 or 16 times | `interface.loupe-magnification` |
| `Ctrl+=`, `Ctrl++` | Increase interface scale; the picture is not affected | `interface.scale.up` |
| `Ctrl+-`, `Ctrl+_` | Decrease interface scale; the picture is not affected | `interface.scale.down` |
| `Ctrl+0` | Reset interface scale to the monitor's own | `interface.scale.reset` |
| `x` | Select a region of the image; again, or `Esc`, removes it | `region.select` |
| `.` | Cycle how the pixel under the pointer is read out: hex → decimal → mapped → depth | `interface.pixel-format` |
| `,` | Cycle where the pixel is read out as, among what the file can say: pixel → projected → geographic → direction | `interface.coordinate-format` |
| `<` (`Shift+,`) | Switch a latitude and longitude between decimal degrees and degrees, minutes and seconds | `interface.geographic-format` |
| `` ` `` | Show or hide the panels around the image | `interface.toggle` |
| `~` | The same, and closes the histogram, information, minimap and file list | `interface.toggle-panels` |
| `?`, `/` | Show the keys: every one of them, what it does and when. Again, `Esc` or a click outside closes it. The button at the foot of the right strip does the same | `interface.help` |
| `Ctrl+,` | Open the configuration file in your editor, writing it first if there is none. The button at the foot of the help popup does the same | `interface.settings` |
| `q`, `Esc` | Quit. `Esc` closes a popup, a message or a region, or brings the panels back | `interface.quit`, `interface.dismiss` |

The interface's size steps through 75%, 100%, 125%, 150%, 175%, 200%, 250%
and 300% of what the monitor's own scale gives it, and is remembered from one
run to the next. Use it when the panels' text is too small or too large on
one monitor without changing every other program's: the picture stays where
it is, and 100% zoom is still one image pixel to one screen pixel. The window
keeps its size when you change it.

The panels are opaque and the image is fitted inside them, so hiding them
gives a fitted image more room and it re-fits immediately. The file list stays
when they go, without the buttons at its head, and runs down the whole left
edge of the window; `~` closes it with the rest.

The button in the top right corner hides them too, and holding Shift as you
press it does what `~` does. The first time they go, a message says which keys
bring them back: `` ` `` and `Esc`. It appears once — after that you know — and
`Esc` brings the panels back before it does anything else, so pressing it while
the message is still up puts them on screen rather than only taking the message
off.

The histogram's bars are linear. Display and Image share one scale, so a
bar of the same height holds the same share of the picture in both, and the
scale stays put while you change the display. Output is scaled to fill its
plot whatever the display does to the picture. A spike that holds far more than
the rest — a flat sky, the black surround of a scan — runs off the top
rather than flattening everything else. It is capped in the accent color,
and with the pointer on it the section's heading says what share of the
picture it holds.

Under the plot are the band and the rows of settings the plot is drawing,
described under [The display](#the-display) above: the exposure, the
window and the curve, for every file. The buttons of the last two rows set
rather than switch, so pressing a window's again after moving the window by
hand puts it back where it says.

The histogram, the file information and the minimap float over the image
rather than sitting in the bars, so `` ` `` leaves them where they are.
`Shift` with it closes all three as well, for the image on its own; they
stay closed when the bars come back.

The histogram and the file information are each a fixed width, and the
histogram a fixed height as well, so a small enough window has nowhere to put
them. Their buttons go dim when it has not, and pressing one does nothing;
resting on it says why. Making the window larger brings them back. A window
opened for a file is never smaller than the two of them need, so this is
something you meet after resizing rather than on opening — unless the screen
itself has no room, in which case `--size` is the way past it.

The file information sits down the right of the image, under headings, so
that a long column can be read by looking for a thing rather than from the
top. [METADATA.md](METADATA.md) lists every section and field it shows.

Clicking copies. A click on a field copies what it says; a click on a heading
copies that whole section, one line per field as a name and a value; and the
button at the top of the panel copies the lot, with the section named on each
line as well — the same thing `Ctrl+I` does. A mark appears over whatever the
pointer is on to show what a click would take. It goes away while the column
is being scrolled, since the pointer is then resting rather than pointing.

Where the panel has more to say than fits, the wheel scrolls it — point at
the panel rather than at the image, and the wheel moves the words instead of
the zoom. Dragging the panel scrolls it
as well, the drag holding the scrollbar's handle rather than the words: drag
down to move down the column, and a short drag carries a long column a long
way — as far as putting the handle there would. The pointer belongs to the
panel while it is over it, so neither gesture reaches the image behind.

The grid divides the image into squares of a round number of image pixels —
1, 2, 5, 10, 20, 50 and so on — chosen so that the lines land about fifty
screen pixels apart at whatever zoom the view is at. The button at the head of
the bottom bar says which spacing is in force, in the reading beside its mark,
so a distance on screen can be counted off in the image's own pixels.

The horizon grid, `b` or the compass button after the grid's, is for a
picture whose camera recorded which way it faced — see the *Orientation*
section in [METADATA.md](METADATA.md#orientation). It draws the true horizon
across the picture, and upright lines a set number of degrees apart round the
camera, each running toward the point where everything upright in the scene
meets. Curves the same number of degrees apart above and below the horizon
cross them, each through everything that far above or below eye level, so the
grid reads as degrees both ways. Where the file records which way the lens pointed on the compass, the
uprights fall on round bearings and each is labeled with its bearing over the
horizon. While the pointer is on the picture, two solid lines go through it:
the upright it stands on, and the curve of everything as far above or below
eye level as it is — a curve, since that is how a lens shows it. `B` steps
the spacing through 5, 10, 15 and 30 degrees, and the button reads out the
spacing in force.

Turning the horizon grid on also reads the pointer's place out as a
direction, where the readout was showing the pixel's column and row: its
bearing — or, without a compass reading, how far right of where the lens
pointed — and how far above level, in degrees. Turning it off puts the pixel
back. `,` reaches the direction too, without the grid.

The grid needs the focal length as well: as a 35 mm equivalent, or as the
lens's own focal length with the size of the sensor, which most cameras that
leave the equivalent out record instead. Without either it draws only a fainter line through the middle that shows which way is
level, and the readout cannot give a direction. A compass in a phone is
often off by five to ten degrees, which moves every bearing by the same
amount; a wide lens bends straight edges near the frame away from the lines;
and a picture cropped after it was taken has its middle in the wrong place,
which moves the horizon. Differences between two places in the same picture
are good to about a degree.

A picture that records its compass heading but not how the camera was
tilted, as a Pixel's does, gets only a ring at its middle with the heading
over it. Nothing else in such a picture can be given a bearing, since a
camera pointed up or turned on its side moves every other point's bearing
by an amount the file does not say. `B` does nothing for such a picture.

A bearing from magnetic north rather than true north — as a phone often
records it — has an M after it, on the grid, the ring, the *Orientation*
section's diagram and the pointer's readout, and is always written in
degrees rather than as N, E, S or W. A copy of the readout leaves the M
out.

The loupe, `l` or the button beside the grid's, rings the pixels around
the pointer and shows them magnified in a circle beside it, for looking
closely at one place without zooming the whole view in. The glass sits up and to
the right of the pointer, and moves to the other side where that would take
it off the picture's area. It shows the picture as the screen shows it —
the window, exposure, curve and false color all reach it — and it is there
whenever the pointer is over the image, through a drag as well. Holding the
right mouse button on the picture puts it up as well, for as long as the
button is held, whether or not the toggle is on; and the wheel, while the
button is held, steps the magnification through 2, 4, 8 and 16 times, as
`Shift+L` cycles it. The
glass stays the same size, so the ring around the pointer shrinks as the
magnification grows to what still fits in it, and the button reads out the
magnification in force while the loupe is up.

## Selecting a region

`x`, or the button under the open button in the left strip, asks for a
region: the cursor becomes a crosshair over the image, and the next drag on
it draws a rectangle. It is made of the image's own pixels — every pixel the
drag touched, at whatever zoom you drew it — and it stays on screen, outlined
in the accent with a handle at each corner, in the middle of each edge and at
its center, until you take it off.

Dragging a handle on an edge or a corner moves that edge, or that corner,
and a handle pulled past the far side flips the rectangle over rather than
shrinking it to nothing. Dragging the handle at the center moves the whole
region, and so does dragging anywhere inside it with `Shift` held. Dragging
anywhere else on the picture pans, as it always does — inside the region
too, without `Shift`, so a region that fills the window does not pin the
picture under it; the wheel zooms as before.

For the last pixel, use the keys. One handle is always the current one,
drawn in a brighter color than the others, and the arrows move it one pixel
rather than stepping to another file. A region just drawn has its center handle
current, so the arrows move the whole of it; click or drag any other handle
and it becomes current instead, and the arrows move that: click the right
edge's handle and press Right to make the region one pixel wider, or Left to
make it one narrower. An arrow the current handle cannot follow — Up, on the
right edge's handle — moves the whole region. Click the center handle to go
back to moving the whole. `Ctrl` with an arrow grows the region on that
side, whichever handle is current, and `Ctrl`+`Shift` with an arrow shrinks
it that way, pulling the far side in: `Ctrl`+`Shift`+Left moves the right
edge one pixel to the left. A region never shrinks past one pixel. `Shift`
with an arrow is not the region's: it pans the picture under it, as it does
with no region up. To step to another file while a region is up, use `[`,
`]`, `Page Up` or `Page Down`.

Point at the region and it says what it is: its size under the handle at
its center, `640 × 480`, and the coordinate of each edge written just inside
the mark in the middle of that edge. The left and right numbers are where those edges
sit across the image and the top and bottom where they sit down it, so the
right minus the left is the width beside them. They stay for as long as the
pointer is on the region — including while you are dragging it — and go when
you point somewhere else, leaving the outline. A region too small to hold
all of it writes what fits, dropping the edges before the size.

`Space` fits the region before the image: the whole of it in the window,
then the window filled with it, then the image's own two fits and its actual
size, and round again. Drawing or moving the region starts over at fitting it. A fit of the
region is a zoom like the ones on the number row rather than a fit the view
keeps, so resizing the window does not re-fit it, and a region of a few
pixels stops at 6400%. `Ctrl+C`, and the
**Image** item of the copy menu — which reads **Region** while one is
selected — copy the region instead of the whole picture, with the display
settings applied exactly as they would be to the whole.

`x` again, or the button, takes the region off. So does `Esc`, once any
message is off, and before it quits. Stepping to another file leaves the
region behind: it belongs to the picture it was drawn on.

### The region panel

While a region is asked for or on screen, a small panel stands at the top
left of the picture, beside the region button. Its head has three buttons:
one fits the region in the window, as the first press of `Space` does; one
copies it, as `Ctrl+C` does; and the cross takes it off, as `x` does. The
first two do nothing until a region is drawn.

**Aspect** holds the region to a shape. Open the menu at the end of its box
for **Free**, which is any shape and where it starts; **Original**, the
image's own shape; or one of 1:1, 5:4, 4:3, 3:2 and 16:9. Or type a ratio
into the box: `16:10`, `16/10` and `16x10` all work, as does one number such
as `2.39`, which means 2.39 to 1. Choosing a shape while a region is on
screen reshapes it about its center, keeping about the same number of pixels.

While an aspect is chosen, everything that changes the region keeps it to
that shape. A new drag reaches from where it started as far as the pointer
has gone along whichever side it has gone further along. A corner's handle
keeps the opposite corner where it is. An edge's handle moves that edge,
and the two edges beside it follow, keeping the region centered across.
The arrows with `Ctrl` or `Ctrl`+`Shift` grow and shrink it a pixel at a
time along the side they move, and the other side follows. A region held to
a shape stops at the edge of the image and gives up size there rather than
shape.

The button after the menu swaps the region's width and height about its
center, and swaps the aspect with them: 3:2 becomes 2:3. The menu then
offers its ratios the tall way up. Rotating the picture turns a chosen ratio
with the region, so the region keeps its shape. **Original** follows the
image instead, whichever way it is turned and whichever file is on screen.

The aspect stays chosen when the region is taken off and when you step to
another file, until you choose another or quit.

**X** and **Y** are where the region's top left corner is in the image, and
**W** and **H** its width and height, all in the image's own pixels. Type a
number into one and press `Enter` or `Tab`, or click elsewhere, to apply it.
`Esc` puts the box back as it was. A new position moves the region without
resizing it. A new width or height keeps the top left corner where it is,
and with an aspect chosen the other side follows. A region does not reach
past the edge of the image, so a number too large for the room there stops
at that edge.

## The mouse

| Action | What it does | Slot |
| --- | --- | --- |
| Drag | Pan, with the image following the pointer, until its edge reaches the middle of the window — room to pull an edge out from under something covering it; with a region selected, draw it, or pull one of its handles — the one at its center moves the whole of it | `gesture.image.left.drag` |
| `Shift`+Drag | Inside the region, move the whole of it | `gesture.image.shift+left.drag` |
| Click a region handle | Make it the current handle, the one the arrows move | |
| Double-click | Actual size, and from actual size back to the whole image | `gesture.image.left.double-click` |
| `Space`+Drag | Zoom to the box dragged out, wherever the drag begins | |
| Wheel | Zoom about the pointer | `gesture.image.wheel` |
| Trackpad scroll | The same, by fractions of a notch | `gesture.image.scroll`, or where it has none, `gesture.image.wheel` |
| Click a panel button | Show or hide the histogram, the file information, or the minimap | |
| Press the minimap | Center the view on the point pressed, as near as the image's edges allow, the moment the button goes down | `gesture.minimap.left.drag` |
| Drag the minimap | Move the view with the pointer, keeping the marked-out part of the map under it | `gesture.minimap.left.drag` |
| Click the grid button in the bottom left | Show or hide the grid | |
| Click the compass button beside it | Show or hide the horizon grid | |
| Click the loupe button after that | Show or hide the loupe, a magnified circle beside the pointer | |
| Hold the right button on the picture | Show the loupe for as long as the button is held | `gesture.image.right.hold` |
| Wheel while holding the right button | Step the loupe's magnification: 2, 4, 8 or 16 times | `gesture.image.right+wheel` |
| The back and forward buttons on the side of the mouse | Back and forward in image history, as `Alt+[` and `Alt+]` do | `gesture.image.back.click`, `gesture.image.forward.click` |
| Click the copy button | Open the menu of copies: the file, or the image | |
| Click the region button | Select a region, or take the selected one off | |
| Click the paste button | Paste the image on the clipboard, as `Ctrl+V` does | |
| Click **Open files…** or **Open folder…** in an empty window | Put up the desktop's file dialog, as `Ctrl+O` and `Ctrl+Shift+O` do | |
| Click **Open all in** a folder in an empty window | Open every image in the folder the last picture came from | |
| Click the play, back or forward button under an animation | Play or pause it, or step a frame, as `Enter`, `N` and `n` do | |
| Click or drag along the timeline | Go to the frame under the pointer, and stop there | |
| Click a histogram button | Set the window on one of its rules, choose the tone curve, or mark the clipped pixels on the picture | |
| Drag a handle on the histogram's band | Set the black point or the white point: the value that comes out black, or white | |
| Drag the band between the handles | Slide the window along the axis without changing its width | |
| Click or drag the exposure's slider | Set the exposure to the quarter stop under the pointer | |
| Wheel over the file information | Scroll it | |
| Drag the file information | Scroll it, as if dragging the scrollbar's handle | |
| Click the zoom percentage | Open the zoom menu: scale, print size, fit and the magnification filter | |
| Click the dot beside the grid button | Choose how a pixel's value is read out | |
| Click a turn button, near the bottom right | Turn the picture a quarter counterclockwise or clockwise, as `;` and `'` do | |
| Click the camera button reading **RAW** or **JPEG**, near the bottom right | Show a raw as the camera's JPEG of it, or as the developed picture, as `v` does | |
| Click the axes button, near `HDR` at the bottom right | Show a picture's depth map in its place, or the picture again, as `D` does | |
| Click the sun button, just before `HDR` at the bottom right | Show a picture's gain map in its place, or the picture again, as `G` does | |
| Click the four-square button, near `HDR` at the bottom right | Show a raw's mosaic in its place, or the developed picture again, as `M` does | |

The button at the top of the left strip opens a menu of the
copies beside it: the file's **Name**, its **Path**, its **URI**, the
**Info** the file information panel holds about it, and the **Image** itself.
Each cell does exactly what its key does, and resting on one names the key as
well, so the menu is also where those keys are learned. The two copies
of the pixel under the pointer are not on it: the pointer is over the menu
while the menu is open, so there would be no pixel under it to copy. Pressing
anywhere outside the menu, or `Esc`, closes it without copying anything.

The percentage in the top bar, just inside the button that hides the
interface, is itself a button. Pressing it opens a menu hanging under it, under four headings.
**Zoom** is 10% through 1600%. **Physical** holds **Print size**, which shows
the picture at the size the file says it prints, on the display the window is
on: an inch of the print is an inch of the glass. It needs the file to say how
many pixels go to an inch, as the info panel's *Density* row shows, and the
display to report how large it is, which most do; resting on a dead **Print
size** says which is missing. A display that misreports its size — some
televisions and projectors do — shows the print at the wrong size. A window
straddling two displays is sized for the one holding most of it. **Fit** is the two fits — the whole image, or
the window filled by it. The chevrons on the second point the way it fills,
across the window or down it, which follows the shape of the image.
**Up-scaling** is the filter the image is magnified with, `Nearest` or
`Bicubic`, the same choice `p` cycles. Whichever of each the view is in is
lit, and choosing acts at once; pressing anywhere outside the menu, or `Esc`,
closes it without changing anything. A window too small to hold the menu does
not open one.

Pointing at the image reads that pixel out in the bottom left corner: its
coordinates, a swatch of the color it comes out on screen, and its value. The
swatch is what the display settings actually make of the pixel, false color
included; the coordinates are padded to the size of the image, so nothing
after them moves as the pointer crosses a power of ten. In a window too narrow
for all of it, the coordinates stay.

One pixel answers more than one question, so the value is written whichever of
four ways you ask for. It starts in hex, and after that in whichever you
last chose. **Decimal** is the numbers the file holds, in its own
units — codes for an 8-bit image, counts for a 16-bit one, the value itself
for floating point — the numbers whatever wrote the file put there. **Hex** is
those same numbers as a color is usually written down: run together, in upper
case, with no `#` and no `0x`, two digits to an 8-bit sample and four to a
16-bit one, so an ordinary photograph reads `E78040` and pastes straight into
anything that takes a color. A floating-point file has no such code, and
what you get there is the bits it actually stores. **Mapped** is what the
window, the exposure and the tone curve have made of the numbers, where 0 and
1 are the ends of the window the bar names on the right. **Depth** is how far
away the pixel was, for a picture that carries a depth map beside it, as a
phone's portrait may: a distance, such as `1.372 m`, where the file says how
its map is scaled — `≈0.92 m` where the file calls it an estimate — and
otherwise the number the map holds there, which only says what is nearer
and what is farther. A picture with no depth map reads
`(no depth)`.

`.` steps through the four, and the dot at the head of the readout opens a
menu of them; whichever is in force is lit. It applies to whichever image is
on screen and stays as you set it, into the next run too.

A georeferenced raster — a scanned map, an elevation model, a satellite scene —
can say where the pixel is on the ground as well as where it is in the image,
and for one of those the same menu has two more rows. **Coordinate** chooses
between **Pixel**, the column and row as for any image; **Projected**, the
file's own coordinates — an easting and northing in its coordinate system and
its units, `(2600000, 1200000) m`; and **Geographic**, latitude then longitude
on WGS 84, the system a GPS and a web map use. **Latitude and longitude**
chooses how those are written: **Decimal**, signed degrees,
`46.951083, 7.438632`, or **DMS**, degrees, minutes and seconds with the
hemisphere, `46°57'03.90"N 7°26'19.08"E`. Decimal degrees always have six
places and seconds always two, and either is the middle of the pixel under the
pointer. Projected coordinates are written to about as many places as one
pixel is worth.
`,` steps through the coordinates and `<` switches between the two ways of
writing a latitude.

A file offers only what it can answer. One in latitude and longitude already
has no projected coordinates to give; one whose coordinate system is spelled
out in parameters rather than named, or is named by a code `gamut` does not
know, keeps its own coordinates and offers no latitude. The conversion to
latitude and longitude is good to a meter or two, not to survey accuracy: it
shifts between datums by a fixed offset rather than by the grids a surveyor's
software uses. For every other image the rows are not in the menu at all, and
wherever a file cannot answer what you chose, the readout shows the pixel —
the choice stays, and applies again to the next map you open, in the next
run as well.

The slot in the last column is the name of that gesture in the
[configuration file](SETTINGS.md#gestures), which can give it something else
to do, and give the middle button, the side buttons, and the wheel with a
modifier or another button held, things to do that they do not do by
default. What is fixed, whatever the file says: a drag begun while a region
is asked for draws it, one from a region's handle pulls the handle, one
with `Space` — or whatever `zoom.fit` is bound to — held draws a box to
zoom to, and a click on a handle
makes it the current one.

The pointer keeps its grab until the button comes up, so a drag that leaves
the window goes on working. The cursor becomes a closed hand only when there
is somewhere to drag to.

The panels come between the pointer and the image. Anything on screen over
the image — the histogram, the file information, the minimap, an open menu —
takes what the pointer does while it is over it, so a click there never starts
a drag of the image behind, the wheel there never zooms, and the readout in
the bottom bar goes quiet rather than naming a pixel the panel is covering.
Hide a panel to get that ground back, or point at the image somewhere else.

## Starting somewhere other than the default

Most of what these keys reach can be set before the first file opens, which is
what scripting wants and what comparing two files on equal terms needs:
`--exposure`, `--window`, `--tone-map`, `--colormap`, `--upscale`,
`--histogram` and `--info`. The minimap starts on; `--no-minimap` starts
without it. An animation starts playing; `--paused` starts it stopped.
`--paste` opens on the image on the clipboard, as `Ctrl+V` would once the
window was up. No path at all opens an empty window, with the buttons that
give it something — see [Opening something else](#opening-something-else).

The window itself opens at the image's size, shrunk to fit the screen, and
no smaller than half the screen's height on each side. The room for the
picture is at least as wide as it is tall, so a folder that starts with a
portrait picture still has the width for the landscape ones after it.
`--size <W> <H>` opens it at a size you choose instead, in the pixels your
desktop measures windows in. A tiling window manager takes it as the size to
use when the window floats, and may lay the window out its own way regardless.

## Keys that are deliberately ignored

Apart from the chords in the tables above, anything held with `Ctrl`, `Alt`
or a `Super` key does nothing here unless the configuration file binds it,
and neither does the wheel with one of them held. Those combinations belong
to the window manager, and a chord such as `Super+0` would otherwise move the
view behind its back. On a Mac the same holds for `⌃`, `⌥` and `⌘`
beyond the chords above: `⌃` with the arrows is Mission Control's, and
`⌘H` and `⌘Tab` are the system's.
