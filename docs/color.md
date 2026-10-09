# Color management

The one invariant everything else follows from:

> **A sampled texel is always linear in the working space** — because the data
> was linear already, because the format's hardware decode produces it, or
> because we linearized on the way in.

It matters because a format's own decode — sRGB or otherwise — happens as part
of reading a texel, before anything is weighted against anything else, while a
decode written into the shader necessarily happens after. Decoding in the
shader would mean resampling in encoded space — the classic gamma-incorrect
downscale — and this viewer minifies constantly, since fit is the default. So
the transfer function is resolved once per image at upload, never per frame,
and every weighted sum in [resampling](resampling.md) is over linear light.

The working space is linear BT.709 in `Rgba16Float`, with room above 1.0 for
HDR content to survive until tone mapping.

| Source | Stored as |
| --- | --- |
| 8-bit sRGB color | `Rgba8UnormSrgb` — hardware decodes, filtering stays correct |
| 8-bit sRGB gray | `R16Float` — there is no `R8UnormSrgb`, so a LUT linearizes it |
| 16-bit linear | `R16Unorm` / `Rgba16Unorm`, or half float where unsupported |
| 16-bit encoded | half float via a 65536-entry LUT |
| 32-bit float | `R32Float` / `Rgba32Float`, or half float if the GPU cannot filter them |

The two falls to half float in that table are the only places a picture loses
precision it had. The upload says which it was as a `Reduced`, and the
application puts it in the info panel's *Precision* row for that picture and
in a warning toast the first time each reason comes up in a run — every file
of the kind loses the same on the same device, so it is said once. Nothing
about the texture format is shown otherwise: which format holds a picture
that lost nothing is the renderer's business.

Alpha is coverage, never light, so it is never put through a transfer
function. Three-channel data is expanded to four because no graphics API has a
three-component sampled texture; one- and two-channel data is *not* expanded,
so a 20000×20000 16-bit gray scan costs 800 MB rather than 3.2 GB.

Every repack in `render/upload.rs` — the expansion, and the two LUT and
`to_linear` passes — goes through one `repack`, which divides the pixels
between rayon's threads. The expansion is a copy, but a copy of hundreds of
megabytes: widening a 14000×9600 RGB file to RGBA took 260 ms on one thread,
more than the GPU's own `write_texture` of the result, and the widened buffer
is zero-allocated rather than filled so that its pages are first touched by
the threads writing them rather than by a fill on one.

## Display-referred, scene-referred, and measured

Everything about how a file opens follows from one fact about it: what its
numbers, once they are linear light, are referred to — a **reference
white**, a scene, or nothing. `DecodedImage::referred` says which, and the
transfer function decides it unless a decoder knows better:

- **Display-referred** (sRGB, gamma, PQ, HLG, a photograph with a gain
  map, a developed raw) has been graded by whoever made it. 1.0 is
  white, and anything above it is the highlights they put there on purpose
  — or, with a gain map, the lift the display's room lets through, see
  [formats](formats.md#gain-maps-ultra-hdr-jpeg-and-an-iphones-heic).
  The window stays at 0..1; touching it would second-guess them, and for the
  HDR curves it would move white to wherever the frame's brightest pixel
  happens to be, differently for every frame of a sequence.
- **Scene-referred** (Radiance and OpenEXR, the two formats that carry
  nothing but light) has no white but is light all the same: a render, a
  light probe, a plate, a merge of exposures, in a renderer's units or in
  cd/m², and spread over more stops than a surface has. No window fits it,
  and the trimmed one is sized for the light sources — Debevec's memorial
  church has a median luminance of 0.03 and a 99.9th percentile of 40, so a
  window to the percentile puts 97% of the picture in the bottom code. It is
  **metered** instead, the way a camera exposes a scene: the window stays
  0..1 in the file's own units and the exposure is set so that the key of
  the scene — the geometric mean of its light, `Stats::key` — lands at
  middle gray, `display::MIDDLE_GRAY`, 0.18. The key is read between two
  trims, `stats::KEY_TRIM`, the darkest and the brightest five percent of
  the lit pixels left out: the brightest are the light sources, which are
  the curve's, and the darkest can be a floor of near-nothing — a render's
  residue where a bounce all but died — that would pull the geometric mean
  down by however much of the frame it covers, since a pixel twenty stops
  under the key weighs twenty stops in the mean. Pixels at zero, and
  transparent ones, are not lit at all and are left out before the trims.
  What the meter leaves above white is the tone curve's, as on a graded
  HDR picture. The meter is on the exposure rather than the window because
  that is what it is: the panel's two dials stay two, the slider shows the
  decision in stops, and `d`/`f` and `--exposure` move on from it; a window
  rule asked for on the command line takes the meter's place, since it has
  put the scene's own range at 0..1 already. A Radiance picture that states
  an `EXPOSURE=` other than 1 has been scaled to be looked at already —
  `pfilt` writes the line after scaling, and writes none within two percent
  of 1 — and is display-referred, opening as stored. `EXPOSURE=1`, which
  Blender's own writer put on every picture it saved, says nothing was
  done, and the picture is metered like one that says nothing.
- **Measured** (every other linear file: 16-bit and float TIFF, a
  linear-declared PNG or JPEG XL, sensor counts) may not be light at all —
  an elevation model, a mask, a temperature grid — so neither a white nor a
  middle gray means anything for it. It gets the trimmed window
  (`AutoWindow::Percentile`, the central 99.8%), because 12-bit data in a
  16-bit container occupies a sixteenth of the nominal range and shows as a
  black rectangle otherwise. The full range is a stop on the `e` cycle
  rather than a default, since one hot pixel is enough to ruin it. Linear is
  taken as measured by default: it is the safe reading of numbers nobody
  has vouched for, and only a decoder that knows its format carries light
  says otherwise.

`--transfer linear|srgb|pq|hlg|gamma:N` overrides the guess, which matters
most for TIFF: the same container carries scanned photographs and frames of
sensor counts, and the header does not distinguish them. Window bounds are
reported in source units for linear integer data, so a 12-bit scan reads
`0–4096` rather than `0.000–0.063`.

## Above white: the surface, and the tone map

The working space keeps values above 1.0, and what becomes of them is decided
once, in the compositor, by two things.

**The surface.** An sRGB surface stops at white; an HDR one — scRGB
(`Rgba16Float` + `ExtendedSrgbLinear`) by preference, HDR10 (`Rgb10a2Unorm` +
`Bt2100Pq`) otherwise, the latter converted to BT.2020 on the way out — has
room above it, and shows the highlights at the brightness they were graded to.

Which surface the window gets follows the monitor. A driver reports an HDR
color space whether or not the monitor in front of you is HDR — wgpu's
`display_hdr_info` comes back empty everywhere but Windows and macOS — but on
Wayland the compositor knows, and says: every output carries an image
description under the color-management protocol, and `monitor/wayland.rs` reads
them on a connection of its own and listens for changes (the same connection
reads each monitor's room for the opening window — see
[interface](interface.md)). A monitor in HDR
mode gets the HDR surface, one in SDR mode gets the sRGB one, and a window
carried from one to the other switches on the way. Asking the other way
round is what a compositor answers with a modeset: Hyprland switches a
monitor into HDR mode when a fullscreen HDR surface lands on it, and on the
NVIDIA driver that switch blanks every display for a second. So the surface
never leads. `--output hdr` is the one request that still does, for anyone who
wants exactly that switch; `--output sdr` stays on the sRGB surface whatever
the monitor is.

`o`, or the `HDR` button at the end of the bottom bar, is then a switch for
the room rather than the surface: on a monitor in HDR mode it turns the
headroom off and on, and the compositor clips at white in between, with the
surface left where it is so that the compositor is asked for nothing. It is
drawn dead, and takes neither a press nor `o`, on a monitor in SDR mode or
where no HDR color space is offered for the window; resting on it says which
of the two it is, and the first of them names `--output hdr` as what would
change the answer — see `ui::tooltip::disabled`, which reads `App::hdr_state`
for the same answer the button is drawn from. Nothing is written to the
terminal, since a switch that explains itself where the pointer is has no
reason to explain itself where the window is not. Off Wayland, or under a
compositor without the protocol, nothing says what the monitor is, and the
switch moves the surface itself, as the only lever there is —
`Monitors::speaks_modes` is how `App::hdr_state` tells that case from a
monitor that has simply not been described yet.

On a Mac, `NSScreen` says the same: a display whose potential headroom is
above one is in HDR mode, and its current headroom is what the gain map is
weighed against. wgpu's Metal surface offers the scRGB pair and sets the
layer's extended-range flag and color space itself. Asking for the room never
switches a Mac's display into another mode, so following the monitor is
cheap there rather than necessary; see [macOS](macos.md#what-differs-and-why).

**The tone map** is a curve *added* to fit values above white into a surface
that stops there: `neutral`, or `none` — which is not a second curve but the
absence of one, and means whatever the surface does on its own: a clip at
white on SDR, a pass-through on HDR. So the same two-way choice means the
same thing on either surface, and "what would this look like in SDR" is the
surface switch rather than a third stop for `t`.

There is one curve because a viewer wants exactly one thing of it: the
highlights back under white and everything else left where it was, which is
what Khronos PBR Neutral does — below its shoulder a value comes out as
itself. A curve that re-grades the in-range picture to make room for them —
Reinhard's `c / (c + 1)` sends white to a half, and on an 8-bit file at 0 EV
changes every pixel — is the editing the [histogram panel's own
line](histogram.md) rules out, and a second curve that only ever showed a
worse rendering of the same highlights would be a choice with nothing to
choose. The panel's row calls the two *Clip* and *Roll off*, since that is
what the choice is.

A picture opens with no curve, on either surface, and a curve comes on only
by `t`, the panel's row or `--tone-map`. The one curve there is changes the
whole picture — its toe takes an offset out of every shadow, and its
shoulder starts at 0.76 — so a curve that came on by itself would render
two files in a folder differently on whether a specular reached a hair past
white, and would move every P3 photograph with a few pixels outside sRGB
(see **Wide gamut** below). What the surface throws away is said instead:
`Display::exceeds_white` — whether the window and the exposure leave
anything past white, a question about the display rather than the file —
is what the bottom bar's **clipped**, the histogram's corner and the marks
are drawn from. A file keeps the curve it was left in (`app/kept.rs`), and
the panel's reset takes it off with the rest.

A photograph with a gain map has nothing above white on an SDR surface
anyway: its lift is weighed by the surface's room, and the statistics are
scanned through the lift at that weight, off the loop — `App::refresh_lift`,
asked again
when the surface is settled, which is after the first file is decoded and
before the window opens, and whenever the room moves under it.

The bottom bar names what is being done and nothing else: **rolled off** when
the curve is on, and **clipped** when there is none, the surface is SDR and
there are highlights being thrown away — so an ordinary photograph pushed a
stop up says so, and the picture never goes flat at the top in silence.

False color (`r`, or `--colormap`) applies to single-channel images, and
holds the curve at a clip while active, on either surface — a curve on top of
a colormap would distort the mapping you are reading values off, and there is
no color past the end of the ramp for headroom to show as.

**Wide gamut** is headroom of another kind. The working space is BT.709, and
`params.primaries` in `shaders/image.wgsl` carries a P3, Adobe RGB or
BT.2020 file's color into it before the window goes on, so a color outside
BT.709 comes to a channel above white or below zero there: a P3 red is 1.22
in red and −0.04 in green. On an SDR surface the compositor clips both,
which loses the color as clipping a highlight loses it, and the program
reads it as the same loss. `Stats::scan` measures the picture in the working
space — `Values::to_working_space`, the lift and then the matrix, with the
plotted channels put back on the file's own curve — so the histogram's red
plane stands past white and its green below black; `Stats::peak` carries
the highest channel, which is what `Display::exceeds_white` asks, since the
luminance alone never passes white for a pure red of any gamut; the corners
count the share; and `judge` marks the pixels after the same matrix.
`DecodedImage::sample` carries the matrix in `Sample::linear` for the same
reason, so that the histogram's marker lands in the bar the scan counted the
pixel in. The file as it stores the color is measured too, with no matrix
and no lift (`Stats::scan_as_stored`), for the histogram's Image section, so
that the panel shows the P3 red at the top of the file's own range there
and past it only above, where the working space has taken it. On an HDR surface nothing is lost, so `tone_map`'s headroom arm in
`shaders/composite.wgsl` passes the color through untouched, the negatives
with it — scRGB is defined to carry them, and the `Rgba16Float` target holds
them — and the HDR10 arm takes the color to BT.2020, which holds P3 and
Adobe RGB whole, before it clips what is outside that. `ToneMap::apply` is
the CPU twin of all three arms, and `the_tone_curve_on_the_device_is_the_readouts`
in `render/filter_tests.rs` holds the device to it over a sweep that runs
below zero. The neutral curve keeps its clamp on either surface: its toe
reads the darkest channel, and its arithmetic is written for light.

One thing the HDR path does not do, deliberately: a scene-referred file on
an HDR surface is still windowed to 0..1, since without a reference white
there is nothing to put above it — widen the window or raise the exposure to
use the room.

