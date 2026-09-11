# Design notes

How `gamut` works and why it works that way. These pages are for people
reading or changing the code; anyone who only wants to *use* the program
wants [`user-docs/`](../user-docs/) instead.

| | |
| --- | --- |
| [Architecture](architecture.md) | The three rendering layers, how egui fits under the compositor, and where each file's job is |
| [The interface](interface.md) | Why the controls are shaped as they are: the chrome, the layers the pointer is routed by, the information panel, menus |
| [Color management](color.md) | The one invariant, the working space, display- vs scene-referred, and what happens above white |
| [Resampling](resampling.md) | The filters, and the coarse chain that makes minification affordable |
| [Live reload](live-reload.md) | Watching the file and the directory, and why it is a `stat` rather than `inotify` |
| [Animation and pages](animation.md) | The frames a decoder composites, the thread that decodes them ahead under a budget, and the clock they play by |
| [Theme](theme.md) | Reading the desktop's palette, and the two things that resist being themed |
| [Formats](formats.md) | Each decoder, what it can and cannot say, and how to add one |
| [Known limits](limits.md) | What does not work yet, and why |
| [Tests](testing.md) | What is covered, including the eight that run on a real adapter and the interface driven headless |
| [Licensing](licensing.md) | The dual license, third-party work in the tree, and how the notices are generated |
