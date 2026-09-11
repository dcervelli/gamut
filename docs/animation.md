# Animation and pages

A file that holds more than one picture is one of two things, and the two
are handled differently. An animation is frames on a clock: they are meant
to be seen in order, each for a stated time, and they are all the same shape.
A paged file — an ICO's entries, a TIFF's directories — is several pictures
with no clock between them, free to differ in size, depth and layout, and
looked at one at a time by choice. Both get the transport bar; only the
animation gets a play button and a timeline. `Sequence` in
`src/image/sequence.rs` is the distinction, and every decoder answers it from
the header before a pixel is decoded.

## Three pieces

**The decoders composite.** Each animated format has its own rules for how a
frame is laid over the last — GIF's disposal methods, APNG's dispose and
blend operations, WebP's patches on a canvas with a clear rectangle — and
none of them belongs above the decoder. A `FrameSource` hands over every frame
whole, at the canvas size, with those rules already applied: `image`'s
`AnimationDecoder` does it for GIF and APNG, `image-webp`'s `read_frame` for
WebP, and `jxl-oxide` renders keyframes that are whole by construction. So a
frame is a `DecodedImage` like any still, and nothing downstream — the
statistics scan, the upload planner, the pixel readout, the copy — knows it
came from an animation. A source reads forward only, and `rewind` is how it
goes back: a fresh decoder over the same file handle for the `image` crate's
iterators, `reset_animation` for WebP, and an index reset for JPEG XL, which
keeps every frame it has read.

**The player decodes ahead** (`src/player.rs`). A thread of its own owns the
source and a cache of decoded frames, each scanned for its statistics on the
way in so that the histogram follows the frame on screen without the event
loop scanning anything. It is told where the play head is and decodes the
nearest frame ahead of it that is not held, which for a file under the budget
means all of them, playing or paused, and then nothing until the head lands
somewhere it is not. The budget is `MAX_SEQUENCE_BYTES`, a gibibyte: past it
the cache is a window around the head, the frame farthest ahead going round —
which is the one just behind the head — let go first, and a seek back behind
the window sends the source back to the start to read forward to it. One
shape rather than two, so that the small file and the large one are the same
code and the budget alone decides which behavior a file gets; the alternative
of refusing to play a file that will not fit was dropped because a long
high-resolution WebP is exactly the file someone opens this program for.
The event loop is woken through a user event as frames arrive, the way the
loader wakes it for a finished file, since the loop sleeps until something is
due and a frame it is waiting on is something due.

**The clock is pure** (`src/app/playback.rs`). It is told the time and every
delay decoded so far, and answers with the frame that should be on screen
and when the next is due. The frame due is worked out from when the current
one began and how long the file says it lasts, never by counting redraws, so a
redraw that comes late finds the clock moved on past the frame it missed and
the picture catches up rather than slipping a frame behind every time the
loop was busy. What it will not do is run ahead of the decoder: a frame due
but not yet held stalls the clock, and gets its whole delay from the moment
it arrives. A slow decode makes a slow animation, never a jumpy one. The
loop's `about_to_wait` folds the clock's deadline into the same `WaitUntil`
the toast and egui's repaint use, which is the one place the program sleeps.

## What a frame change costs

The frame's pixels are written into the texture the last frame's occupy —
`Upload::refill` in `src/render/image_layer.rs` — rather than uploaded to a
new one, which makes a frame a copy and nothing more. The coarse chain was
reduced from the old pixels and is dropped with them; the next minified draw
builds it again, as it built the first. A frame that is not the same shape as
the texture — which no format here produces, every frame being the canvas
size at the file's depth — falls back to a fresh upload. The refill runs on
the event loop's thread, in `App::show_due_frame` at the head of a redraw: an
8-bit sRGB frame takes the hardware path and costs a copy of tens of
megabytes at most, which is a millisecond. A 16-bit JPEG XL animation pays
the transfer-function table on every frame as well; if that ever shows, the
seam for moving the refill onto the player's thread with two textures is
`Renderer::refill_image`.

The interface's picture and statistics are swapped for the frame's, so that
everything reading `Current::image` — the readout, the histogram, a copy, the
information panel — reads the frame on screen. The display is left alone: a
window or an exposure is a setting, and an automatic window re-derived from
every frame would be a picture that pumped. A reset or a cycle of the
automatic window asked for by name uses the frame that is up.

## Delays

A GIF delay of a hundredth of a second or less is shown for a tenth,
`sequence::gif_delay`: early encoders wrote 0 and 1 meaning "as fast as you
like", every browser settled on 100 ms for them, and a file authored against
that convention plays here at the speed its author saw. Every other format's
delay is taken as stated, and nothing is shown for under `MIN_DELAY`, ten
milliseconds, so a file stating zero cannot spin the clock.

GIF's loop extension is read with the `gif` crate directly rather than
through `image`, which reports a file with no extension as looping for ever
where browsers play it once. The extension's count is how many times to
*repeat*, so a file saying 1 plays twice. The same walk of the file counts
the frames without decoding any, which the header itself does not state.

## Pages

A page is a read like a file: `Request` names it, `Reload::Page` says why,
and the loader decodes it, scans it and uploads it as it would a file, since
a page may be any size or layout. `App::apply` treats it as it treats a file
changed on disk: the display stays, the view stays where the size matches
and is fitted afresh where it does not. `n` and `N` step pages and frames
alike, going round the ends as `]` and `[` go round the list. The page a
file was left on is kept beside its view and display in `kept.rs`, and a
file coming back is asked for at that page. A frame is kept the same way,
with whether it was stopped there: an animation left playing comes back
playing.
