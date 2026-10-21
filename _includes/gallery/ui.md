## Getting Started

Just pass filenames or directories to `gamut`:
```
gamut file1.jpg pictures file2.png
```
Directories are scanned non-recursively.

## Keyboard

Basic keys to get going:

| Key (Linux) | Key (macOS) | Action |
| --- | --- | --- |
| `?` | `?` | Show/hide help screen |
| `]`, `[` or `PgDn`, `PgUp` | `]`, `[` or `Fn↓`, `Fn↑` | Next / previous file |
| Space | Space | Cycle fit → fill → 100% |
| `1` | `1`, `0` or `⌘0` | Actual size; wheel to zoom, drag to pan |
| `h`, `i`, `m` | `h`, `i`, `m` | Histogram, file information, minimap |
| `` ` `` | `` ` `` | Hide the interface |
| `tab` | `tab` | Show/hide the file list |
| `q`, `esc` | `q`, `esc` or `⌘Q`, `⌘W` | Quit |

All features can be keyboard-driven, but the hideable UI also has controls with tooltips to learn keyboard shortcuts. Most controls can also be set via CLI flags.

Open the keyboard shortcut display with `?`. On a Mac, the keyboard shortcuts that use `Ctrl` on Linux generally use `⌘`, and the help shows the macOS keys. Mouse gestures differ too: on Linux the scroll wheel zooms; on a Mac two-finger scrolling pans, and pinching or `⌘`-scrolling zooms.

All keyboard shortcuts and mouse gestures can be customized in the [config file](#config-file).
