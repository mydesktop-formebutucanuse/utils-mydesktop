# utils-mydesktop

Small desktop utilities for X11, written for a Window Maker desktop on
Gentoo (Asahi Linux).

| Tool | What it does |
|---|---|
| [mediakeyd](mediakeyd) | Runs commands for volume, brightness and media keys without grabbing them, so focused windows do not flicker |
| [backlight](backlight) | Changes the screen or keyboard backlight through logind, without root |

## Build

The Rust tools are members of one Cargo workspace. From the top of the
repository:

```sh
cargo build --release
```

Binaries are written to `target/release/`. See each tool's README for
installing and configuring it.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
