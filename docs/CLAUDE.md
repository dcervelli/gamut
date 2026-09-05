# Writing `docs/`

These pages explain the implementation to someone reading or changing the
code. They are the opposite number of [`user-docs/`](../user-docs/), which is
written for people who only run the program — if a sentence would change what
a *user* does, it belongs there, not here.

**Name things by their real names.** Crates, modules, types, file paths,
shader functions, the fixtures in `test_images/`. A reader here has the tree
open, and a description that avoids naming `render/upload.rs` costs them the
search.

**Say why, not just what.** The code already says what it does. These pages
exist for the reasoning that would otherwise be lost: the alternative that was
tried and dropped, the constraint that forced a shape, the bug that a
structure prevents. A paragraph that could be replaced by reading the function
is not worth keeping.

**One page per subsystem, and link across rather than repeating.** A fact
belongs in exactly one page. Where another page needs it, link to it —
`[resampling](resampling.md)` — so the two cannot drift.

**Do not restate the key table, the format table, or anything else
`user-docs/` owns.** Link to it.

**American spelling**, as everywhere else in the project: `color`, `gray`,
`normalize`, `center`, `behavior`.

**Voice: concise, not terse.** Complete sentences, present tense, active
where it reads naturally. State the thing, then the reason it is that way.
