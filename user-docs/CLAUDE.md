# Writing `user-docs/`

Everything in this directory is written for people who *use* `gamut`, not
for people who work on it. The README is where the implementation is
explained; nothing here should duplicate that.

**Write only what a user can act on.** Which files open, what the program does
with them, where it will surprise them, what to press or pass when it does.
If a sentence would not change what a reader does or expects, cut it.

**Never name a library, crate or dependency.** A user does not care which
decoder reads a JPEG, and naming one tells them nothing they can use. Describe
the behavior instead: "12-bit files do not open", not "the decoder is 8-bit
only". The one exception is software the user must install themselves — that
is a prerequisite, not an implementation detail, and it belongs here with the
package names they will need.

**Never point at files in this repository.** No test fixtures, no sample
images, no source paths, no benchmark files. An example must be one the reader
could recognize from their own work — "a 16-bit scanned photograph", "an
elevation model" — not one that exists only in the tree.

**Keep internals out of the vocabulary.** Chunk names, box names, container
layouts and structure sizes are implementation, and reading them costs a user
more than it tells them. Name a thing the user already meets elsewhere — an
ICC profile, an EXIF orientation, a gain map — and describe everything else in
plain words.

**American spelling.** `color`, `gray`, `normalize`, `center`, `behavior` —
the same rule the rest of the project follows, so that a user searching these
pages and a user reading the interface meet the same words.

**Voice: concise, not terse.** Complete sentences, no padding. State what
happens, then why it matters if the why is not obvious. Prefer the active
voice and the present tense. A caveat is worth a sentence of explanation; it
is not worth a paragraph.
