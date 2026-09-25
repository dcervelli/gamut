# Settings

`gamut` reads two files when it starts. One is yours and the other is its
own, and neither has to exist.

## The configuration file

`~/.config/gamut/config` (under `$XDG_CONFIG_HOME` if you set it) says how the
window opens every time. `gamut` only reads it: toggling a panel in the window
lasts until the window closes and does not change the file.

Each line is `name = value`, and `#` starts a comment. Anything left out keeps
its default. To start a file with every setting listed, commented out at its
default, run:

```
mkdir -p ~/.config/gamut
gamut --print-config > ~/.config/gamut/config
```

Then remove the `#` from any line you want to change. A line you leave
commented out keeps following the default if a later version changes it. The
settings are:

| Setting | Default | What it does |
| --- | --- | --- |
| `show_ui` | `true` | The panels around the picture, which `` ` `` hides and shows |
| `show_minimap` | `true` | The minimap, while the picture is larger than the window |
| `show_filmstrip` | `true` | The file list, while there is more than one file |
| `show_histogram` | `false` | The histogram panel |
| `show_info` | `false` | The file information panel |
| `pixel_format` | `hex` | How the pixel under the pointer is read out: `hex`, `decimal` or `mapped` |
| `log_counts` | `false` | The histogram's bars as tall as the logarithm of their counts |

`--histogram`, `--info` and `--no-minimap` override the file for that one run.
If a line has a misspelled name or a value `gamut` does not recognize, it
names the line on the terminal and uses the rest of the file.

## The state file

`~/.local/state/gamut/state` (under `$XDG_STATE_HOME` if you set it) is where
`gamut` remembers settings you change by hand: how wide you dragged the file
list and how far the loupe magnifies. It is written when the window closes
and read when the next window opens. You never need to edit it, and deleting
it resets both to their defaults.
