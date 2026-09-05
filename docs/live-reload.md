# Live reload

## What a path stands for

A directory named on the command line stands for the images directly inside
it, in name order — one level, and chosen by extension, since the alternative
is opening every file in the directory to see what it is. That makes it a
place to look rather than a list fixed at startup, which is what the rest of
this page is about. `src/listing.rs` owns the reading.

## Watching

The file on screen is watched, and a write to it by anything else — a render
finishing, a script rewriting its output, an editor saving — is picked up and
shown within about half a second. Nothing has to be pressed.

A reload keeps you where you were: the same pan and zoom, the same exposure
and tone map, with only an automatic window re-derived from the new pixels.
The point is watching one spot as the numbers under it change. A file that
comes back a different size is treated as a different picture and gets a fresh
fit. `]` and `[` move the watch along with the view.

It is a `stat` every 250 ms, not `inotify`. That costs nothing measurable, and
it is the version that works over NFS and SSHFS and that survives the way most
editors save — a temporary file renamed over the original, which leaves a
watch on the original inode looking at a file nobody will ever write to again.
A change is read only once the size and timestamp have held still for a whole
interval, so a file caught halfway through being written is waited out rather
than decoded and reported as corrupt.

A directory named on the command line is watched the same way and by the same
means — its own `stat`, on the same cadence — and when it changes the list is
read from it again. An image written into the directory joins the walk where
its name puts it; one deleted leaves it. So a script dropping frames into a
folder builds the list as it goes, and `]` reaches a file that did not exist
when the window opened.

Two things hold still through that. The file on screen is never dropped from
the list, whatever has happened to it on disk: its pixels are up and correct,
and everything the interface says about them — the title, the bars, the
information panel — is read off the path they came from. It keeps its
neighbors too, so `]` from a file deleted under you goes on to whatever has
taken its place rather than back over one already seen. And the list is only
rebuilt between reads, since a rebuild moves the file on screen to a new index
and a reply already on its way is aimed at the old one; a change noticed
during a read is simply seen again at a later look.

