# npns
A weak, low-efficient TUI file system browser which is 
developed for poor nerds who want to learn embedded Linux but couldn't even afford an LCD screen (such as me).
Built in Rust, it's a no-frills tool for browsing files over serial consoles or minimal terminals—perfect for cross-compiling kernels on a shoestring budget.

## Preview
![demo](./assets/demo.png)

## Features
- Supports most of the file operation, like Copy, Cut, Paste
- Could handle conflict files while pasting
- Cannot undo `delete`, because Trash dir may not exist
- No mouse required, and arrow keys work too
- Can work on my machine (seriously, I.MX6ULL MINI and ATK-DLAM62L)
- About 1-2 MiB under release mode (ratatui + crossterm)

## Compile

Add the target:
```
rustup target add aarch64-unknown-linux-musl
```

Build:
```
cargo +nightly build --release --target aarch64-unknown-linux-musl
```

For ARMv7 (e.g. I.MX6ULL):
```
cargo +nightly build --release --target armv7-unknown-linux-musleabihf
```

Cross-compilation requires the appropriate linker in `.cargo/config.toml`:
```toml
[target.aarch64-unknown-linux-musl]
linker = "aarch64-linux-gnu-gcc"

[target.armv7-unknown-linux-musleabihf]
linker = "arm-linux-gnueabihf-gcc"
```

## Keybindings
| Key       | Action                  | Notes                                |
|-----------|-------------------------|--------------------------------------|
| /         | Search                  | Search files                         |
| .         | Hide                    | Toggle visibility for hidden files   |
| j   k     | Down / Up               | Cycle rows                           |
| h         | Parent directory        | `cd ..` equivalent                   |
| l   Enter | Enter dir / Select file | Resets selection to 0 on enter       |
| Space     | Select current          | Toggle selection                     |
| c   x     | Copy / Cut file         | To clipboard                         |
| v         | Paste                   | From clipboard to current/target dir |
| d         | Delete                  | Unrecoverable                        |
| n   m     | New file / New dir      | Enter name in input mode             |
| r         | Rename selected         | Pre-fills name in input mode         |
| u         | Undo last operation     | Couldn't undo delete                 |
| Esc       | Cancel input            | Escape hatches everywhere            |
| Q         | Quit                    | Returns to the launch directory      |
| q         | Quit + cd               | cd to current browsed dir on exit    |
