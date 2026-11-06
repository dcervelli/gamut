The File List shows all of the files that navigation and sorting apply to. If specific directories (either directly or with `-r`/`--recursive`) were opened, gamut will [watch](#live-reload) them in addition to the listed files.

### Navigation

Navigation actions are as follows:

| Action | Linux | macOS |
|--|--|--|
| Next file | `→` `↓` `]` `Page Down` | `→` `↓` `]` `Page Down` (`fn+↓`) |
| Previous file | `←` `↑` `[` `Page Up` | `←` `↑` `[` `Page Up` (`fn+↑`) |
| First file | `Home` | `Home` (`fn+←`) |
| Last file | `End` | `End` (`fn+→`) |
| History back | `Alt+←` `Alt+[` `Alt+Page Up` | `⌘[` |
| History forward | `Alt+→` `Alt+]` `Alt+Page Down` | `⌘]` |
| Open [fuzzy finder](#fuzzy-finder) | `Ctrl+P` | `⌘P` |

On a Mac keyboard without dedicated keys, `fn` with an arrow gives `Page Up`, `Page Down`, `Home` and `End`. A mouse's back and forward buttons also step through history.

### Other Actions

| Action | Linux | macOS |
|--|--|--|
| Remove file from list, does *not* delete file | `⌫` | `⌫` |
| Trash file | `Del` | `⌘⌫` `⌦` |

`⌫` is the key labeled Backspace on a PC keyboard and delete on a Mac. `⌦` is forward delete, `fn+⌫` on a Mac laptop.

### Sorting

Press `O` to open the sort order menu, then a second key for the particular types. This is effectively a key chord, `O` then the letter for a type.

| Sort | Key | Description|
|--|--|--|
| Name | `n` | File name, ignoring path |
| Path | `p` | Path, excluding file name |
| Type | `e` | Image type (based on reader, not extension) |
| Modified Date | `m` | File modified date |
| Capture Date | `t` | Image capture date, if available |
| Size | `s` | File size |
| Width | `w` | Image width | 
| Height | `h` | Image height |
| Area | `a` | Image width × height |

Use `R` to toggle sort direction (the sort menu does not need to be open).

Two levels of sorting are supported. Whenever the sort type (not direction) is changed, the old type (with its direction) becomes the secondary sort.
