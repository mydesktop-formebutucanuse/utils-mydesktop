# utils-mydesktop

Small desktop utilities, written for a Window Maker (X11) desktop on
Gentoo (Asahi Linux, Apple Silicon MacBook).

| Tool | What it does |
|---|---|
| [mediakeyd](mediakeyd) | Runs commands for volume, brightness and media keys without grabbing them, so focused windows do not flicker |
| [backlight](backlight) | The brightness keys' command: screen steps through autobrightd, keyboard through logind |
| [autobrightd](autobrightd) | Sets the screen and keyboard backlight from the ambient light sensor, and steps aside when you change them |
| [kernel](kernel) | Asahi kernel patch: the panel backlight in 1/16 nit, so brightness changes have no visible steps |

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
