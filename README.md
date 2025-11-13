# npns
A weak, low-efficient TUI file system browser which is 
developed for poor nerds who want to learn embedded Linux but couldn't even afford an LCD screen (such as me).
Built in Rust with Ratatui and Crossterm, it's a no-frills tool for browsing files over serial consoles or minimal terminals—perfect for cross-compiling kernels on a shoestring budget.

## Features
  - Supports most of the file operation, like Copy, Cut, Paste
  - unable to recursive copy a dir, which meand you still need to copy file by file
  - couldn'd undo `delete`, because Trash dir may not exist
  - need not mouse
  - can work on my machine(seriously I.MX6ULL MINI)

## Compile
Just compile it as how you compile other embedded rust projects. 

and the given config is used for Arm-Linux. Just run the following command

```
cargo build --target armv7-unknown-linux-musleabihf --release

```
## Keybindings
| Key       | Action                  | Notes                          |
|-----------|-------------------------|--------------------------------|
| `j` / `k` | Down / Up               | Cycle rows                     |
| `h`       | Parent directory        | `cd ..` equivalent             |
| `l` / `Enter` | Enter dir / Select file | Resets selection to 0 on enter |
| `Space`   | Select current          | Updates status                 |
| `c` / `x` | Copy / Cut file         | Files only; to clipboard       |
| `v`       | Paste                   | From clipboard to current/target dir |
| `d`       | Delete                  | Magenta confirm: y/N (irreversible) |
| `n` / `m` | New file / New dir      | Enter name in input mode       |
| `r`       | Rename selected         | Pre-fills name in input mode   |
| `u`       | Undo last operation     | Most ops; history capped at 64 |
| `q` / `Esc` | Quit / Cancel input  | Escape hatches everywhere      |
