# Screenshots

The pictures in [`user-docs/screenshots/`](../user-docs/screenshots/) are
made by the scripts in [`bin/screenshots/`](../bin/screenshots/), one script
per picture, so that a picture can be taken again after the interface changes
and come out the same size, in the same place, showing the same thing. What
each one shows is the script: `main_screenshot` opens `~/git/mora` in a
1200×800 window, presses `+` three times and `Up` six, and captures;
`animated` opens a GIF paused in a 1000×600 window, records it playing
through once, and turns the film into the README's `animated.gif`;
`pixel_grid` turns the grid on, rolls the wheel over the stag until the
grid is at single pixels, and presses Space; `info` opens an elevation
model in turbo with the panel up, at 50%.

## How a script takes a picture

[`bin/screenshots/lib.sh`](../bin/screenshots/lib.sh) is what the scripts
share, and it speaks only Hyprland. The compositor has to do three things the
program cannot do for itself:

- **Leave the window at the size asked for.** `--size` is a request, and a
  tiling layout ignores it. `open` starts the program through `hl.exec_cmd`
  with a rule set that floats and centers the window at exactly that size,
  and at full opacity: Omarchy's default rules make every window slightly
  translucent, which would blend whatever is behind it into the picture.
- **Say where the window is.** `hyprctl clients` reports the window's
  position and size in logical pixels, and `grim -g` captures that rectangle
  at the monitor's own scale. On a monitor at scale 1.6 a 1200×800 window
  comes out as a 1920×1280 image, which is the size the README's pictures
  are. The pointer is warped outside the window first, so that neither the
  pixel readout nor a tooltip is in the frame.
- **Type into it.** `wtype` presses keys on a virtual keyboard, and they go
  to whichever window has focus. Focus follows the pointer, so `open` warps
  the pointer into the window before asking for focus, and waits until the
  compositor reports the window as active before returning.

`keys` presses one key per `wtype` call, and the reason is worth knowing
before changing it. `wtype` builds its keymap as it goes: each new keysym is
added and the whole keymap sent to the compositor again, which forwards it to
the focused window. A key pressed under the new keymap before the window has
read it is interpreted under the old one, and `wtype -k plus -k Up` reaches
the program as something other than `+` and `Up`. One keysym per process is
one keymap per process, and every key lands as itself.

The pointer is parked between shots on the middle of the top bar, the one
place it shows in nothing. Over the picture it puts a pixel readout in the
bottom bar; outside the window it takes the keyboard with it, since focus
follows it, and the next key would go to whatever it was over. It is parked
by leaving the window and coming back rather than by warping straight
there, because a warp within the window sends the window no motion event,
and the readout of wherever it last was over the picture would stay in the
bar.

`close` kills the window and then waits for the compositor to forget it. The
next window can be given the same address, and `open` tells the new window
from the ones already open by address, so a script that opens twice in a row
would otherwise take the second window for the first.

## A device of our own

`wtype` has keys and no pointer, and Hyprland can warp the pointer but not
press its buttons or turn its wheel. What the wheel does — zoom about the
pointer — and what a drag does are half the program, and a recording that
never showed them would be a poor one, so
[`bin/screenshots/device.py`](../bin/screenshots/device.py) is a mouse of
its own: a device registered through `/dev/uinput` for as long as one
command runs, sending wheel notches or a held button and motion, then taken
away again. It uses nothing outside Python's standard library; the ioctl
numbers and the event record are written out from the kernel's own headers.
Omarchy gives the user write access to `/dev/uinput` through an ACL, which
is what makes this possible without root. `wheel` and `drag` in `lib.sh`
call it, and `record FILE cursor` keeps the pointer in the film for a
recording where the pointer is the point.

It is a keyboard too, for the bindings `wtype` cannot reach. A binding in
`app/input.rs` is matched either by what the key says (`Char("+")`) or by
where it is (`Position(KeyCode::Digit2)`, which is how `Shift+2` is 50%
on any layout). `wtype` types a keysym under a keymap of its own, at a
keycode it chose, so the window sees the right character at the wrong
position; a key from the device is the real keycode read under the real
keymap, and matches either way. `press shift+2` in `lib.sh` is that. `keys`
stays on `wtype` for everything else, because a keysym is what a binding
by character wants and does not depend on the desk's layout.

The pointer is placed first with `cursor`, and `fitted W H X Y` says where
image pixel X, Y of a W×H image is in the layout while the image is fitted
to the window — the same sum `chrome::content_area` and `View::fit_zoom`
do, redone in awk, so that a script can put the pointer on a feature of
the picture by its own coordinates.

## A recording

`record` starts `gpu-screen-recorder` on the same rectangle, at 60 frames a
second, and waits for the `.ts` file it writes beside the film with its
first frame's wall-clock time. That is the clock the film is cut by:
`since_record` says how long after the first frame something was done, and
`gif` takes a start and a length in those seconds. `animated` reads the
start before it presses `Return` and the length from the file's own frame
delays, so the GIF is one loop of the file from the moment play was
pressed, and loops where the file does. `cut` stops the recorder with
`SIGINT`, which is how it is told to finish the file rather than drop it.

`gif` is ffmpeg's two-pass GIF — a palette from the whole clip, then the
frames dithered against it — at 25 frames a second and 800 pixels wide.
The film itself lands in `target/screenshots/`, which is not in the tree.

## What the scripts are not

They are not tests, and they do not run in CI. They open a window on
whatever workspace is active and type into it, so they want a desk with
someone at it who is not typing anything else. The window the screenshot
shows wears the desktop's theme, as the program always does, so a picture
taken on a different theme is a different picture.
