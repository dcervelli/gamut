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

Alpha is coverage, never light, so it is never put through a transfer
function. Three-channel data is expanded to four because no graphics API has a
three-component sampled texture; one- and two-channel data is *not* expanded,
so a 20000×20000 16-bit gray scan costs 800 MB rather than 3.2 GB.

## Display-referred and scene-referred

Everything about how a file opens follows from one fact about it: whether its
numbers, once they are linear light, have a **reference white**.
`DecodedImage::referred` says which, and the transfer function decides it
unless a decoder knows better:

- **Display-referred** (sRGB, gamma, PQ, HLG, and a JPEG with its gain map
  applied) has been graded by whoever made it. 1.0 is white, and anything
  above it is the highlights they put there on purpose. The window stays at
  0..1; touching it would second-guess them, and for the HDR curves it would
  move white to wherever the frame's brightest pixel happens to be,
  differently for every frame of a sequence.
- **Scene-referred** (linear: sensor counts, EXR, Radiance, float TIFF) has
  no white. It gets a 99.8% percentile window, because 12-bit data in a 16-bit
  container occupies a sixteenth of the nominal range and shows as a black
  rectangle otherwise. Min/max is a stop on the `e` cycle rather than a
  default, since one hot pixel is enough to ruin it.

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
description under the color-management protocol, and `monitor.rs` reads
them on a connection of its own and listens for changes. A monitor in HDR
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
reason to explain itself where the window is not. Off Wayland, or under a compositor without the protocol,
nothing says what the monitor is, and the switch moves the surface itself as
it used to.

**The tone map** is a curve *added* to fit values above white into a surface
that stops there: `reinhard`, `neutral`, or `none` — which is not a third
curve but the absence of one, and means whatever the surface does on its own:
a clip at white on SDR, a pass-through on HDR. So the same three-way choice
means the same thing on either surface, and "what would this look like in
SDR" is the surface switch rather than a fourth stop on the `t` cycle.

What a picture opens with follows from both: none on an HDR surface, and on an
SDR one a `neutral` roll-off where the window leaves highlights above white
and none where it does not. That is a question about the window rather than
about the file — `Display::exceeds_white` — so a PQ frame or a gain-mapped
JPEG opens curved on SDR, and a float measurement raster, which is windowed to
what it holds, opens straight; a curve on it would be a bend in the data for
no reason. Switching the surface asks the question again (`Display::adopt`),
and `t` changes the answer afterwards. The surface is settled after the first
file is decoded — the window opens later — so `App::adopt_headroom` asks once
more at that point too.

The bottom bar names what is being done and nothing else: the curve when one
is on, and `clip` when there is none, the surface is SDR and there are
highlights being thrown away — so an ordinary photograph pushed a stop up says
so, and the picture never goes flat at the top in silence.

False color (`r`, or `--colormap`) applies to single-channel images, and
holds the curve at a clip while active, on either surface — a curve on top of
a colormap would distort the mapping you are reading values off, and there is
no color past the end of the ramp for headroom to show as.

Two things the HDR path does not do, deliberately for now: it clips wide-gamut
color to BT.709 even on scRGB, which could carry the negatives a P3 or
BT.2020 file produces; and a scene-referred file on an HDR surface is still
windowed to 0..1, since without a reference white there is nothing to put
above it — widen the window or raise the exposure to use the room.

