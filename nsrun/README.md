# nsrun

Runs a program in Linux namespaces of its own, with a home of its own, so
that it cannot read or change the rest of the system. With `-x` its
windows are also in an X namespace of their own (XLibre's Xnamespace
extension), cut off from the other windows, the keyboard and the
clipboard.

| File | What it is |
|---|---|
| [nsrun](nsrun) | Runs a command in a compartment, with bubblewrap |
| [nsrun-xserver](nsrun-xserver) | Starts the X server with Xnamespace on, for `nsrun -x` |
| [xnamespaces.example](xnamespaces.example) | The list of X namespaces, for `~/.config/nsrun/xnamespaces` |

On Gentoo the package installs both commands (see the
[top README](../README.md)); by hand,
`install -m 755 nsrun/nsrun nsrun/nsrun-xserver ~/.local/bin/`.

Written for bubblewrap 0.11.2 and XLibre 25.1.11 on Gentoo, X started by
`startx`.

```sh
nsrun web librewolf                 # a browser with a home of its own
nsrun -a -g web librewolf           # with sound and the GPU
nsrun -b ~/Downloads web librewolf  # and the real ~/Downloads, writable
nsrun -n -r ~/paper.pdf pdf mupdf ~/paper.pdf   # no network, one file to read
```

## The compartment

```
nsrun [-n] [-a] [-g] [-x] [-b PATH]... [-r PATH]... NAME COMMAND [ARG]...
```

`NAME` is the compartment. Its home is `~/.local/share/nsrun/NAME`, which
the program sees as `$HOME` and which stays between runs; everything else
it writes is gone when it exits. Programs started with the same `NAME`
share the home, but each run has its own namespaces.

What the program gets:

| | |
|---|---|
| User namespace | The same user, without any capability outside |
| PID namespace | It sees only its own processes; all of them die with nsrun |
| IPC, UTS, cgroup namespaces | Its own; the hostname is `NAME` |
| Network | The real one. `-n`: a network namespace with nothing in it |
| `/usr`, `/etc`, `/opt` | The real ones, read-only |
| `$HOME` | `~/.local/share/nsrun/NAME`. The real home is not there |
| `/tmp`, `/var/tmp`, `/run` | Empty, in memory |
| `/dev` | A minimal one: null, zero, random, tty, pts, shm. `-g`: `/dev/dri` too |
| `/sys` | Not there. `-g`: `/sys/devices` and `/sys/dev/char`, where Mesa finds the GPU's driver |
| Environment | Emptied, except the locale, `TERM`, the input method variables and the cursor theme |
| D-Bus | A session bus of its own (`dbus-run-session`), gone when the program exits. Not the session's bus, and no system bus |
| Sound | None. `-a`: the PipeWire and PulseAudio sockets |
| X | The display's socket and one cookie (see below) |
| `-b PATH`, `-r PATH` | A file or directory of the real system at the same path, writable or read-only |

The program may still make user namespaces of its own, which the
sandboxes of LibreWolf, Chromium and Electron need.

`-a` gives all of PipeWire: the program can record the microphone and what
the other programs play, not only play sound.

The bus of its own is there because a program without one looks the
session's bus up through X, gets an address it cannot reach, and goes on
without D-Bus and with warnings. With no system bus, programs that ask
UPower or NetworkManager something still warn; LibreWolf does about UPower.

Tested in an Xvfb with LibreWolf 159: it runs with its own content sandbox
(seccomp, user namespaces) inside the compartment and keeps its profile in
the compartment's home.

## X: the hole, and `-x`

Without `-x` the program gets the session's cookie and is an X client like
any other. X does not separate its clients: the program can read every
key typed in any window, read the other windows and the clipboard, and
type into a terminal. So against a program that turns on its user, the
compartment without `-x` protects the files only as long as the program
does not go through X. It still keeps a program's files apart, and keeps a
program that merely misbehaves away from the real home.

`-x` closes that. XLibre's Xnamespace extension puts clients into
namespaces by the cookie they give; clients in a namespace see and reach
only each other, each namespace has its own clipboard and selections, and
requests that read the keyboard, the pointer or the screen outside it are
refused. `nsrun -x NAME` gives the program the cookie of the X namespace
`NAME`, and copies the session's X resources (`Xft.dpi`, the cursor theme
and size) to that namespace's root window, which starts empty.

**Most programs do not start in an X namespace yet.** With XLibre 25.1.11,
in an Xvfb:

| Program | In the session's namespace | In an X namespace |
|---|---|---|
| `xrdb`, `xprop` | work | work |
| A GTK 3 window (Python) | works | dies: `BadAccess` on `CreateColormap`, the first of GTK's requests on the root window that the extension refuses |
| LibreWolf 159 | works | dies (segmentation fault) |
| kitty | works | dies: `BadAccess` |

No `allow` word in the list covers these requests. So for now `-x` is for
trying a program out; the X server's log (below) names what it refused.

## Starting X with Xnamespace

The namespaces are fixed when the server starts. `nsrun-xserver` starts it
in place of `/etc/X11/xinit/xserverrc`: put this in `~/.xserverrc`, which
`startx` runs if it is there,

```sh
#!/bin/sh
exec nsrun-xserver "$@"
```

and list the namespaces:

```sh
mkdir -p ~/.config/nsrun && cp nsrun/xnamespaces.example ~/.config/nsrun/xnamespaces
```

At every start `nsrun-xserver`

- takes the cookie `startx` made for the session from the server's `-auth`
  file and makes it the root namespace's, so that the session (Window
  Maker, the daemons, every program not started with `nsrun -x`) is
  unrestricted, as before;
- makes a new cookie for each name in the list and writes it to
  `$XDG_RUNTIME_DIR/nsrun/x/NAME.Xauthority`, for `nsrun -x NAME`;
- writes the server's configuration to `$XDG_RUNTIME_DIR/nsrun/x/conf`
  and starts the server through `/etc/X11/xinit/xserverrc` with
  `-namespace` added.

A client with any other cookie is refused, as before. The directory is
readable by the user only, and nsrun gives a compartment only its own
cookie file.

Without `~/.config/nsrun/xnamespaces` the server starts exactly as
`/etc/X11/xinit/xserverrc` starts it, without the extension. The same if
something is missing (no `-auth` file, no `XDG_RUNTIME_DIR`, no `xauth` or
`mcookie`); `nsrun-xserver` then says why on the terminal.

The extension writes every request it refuses to the server's standard
output, which goes to `$XDG_RUNTIME_DIR/nsrun/x/log`:

```sh
grep -E 'BLOCKED|REJECT' "$XDG_RUNTIME_DIR/nsrun/x/log"
```

If X does not start any more, remove `~/.xserverrc` on the terminal and
run `startx` again.

Tested in an Xvfb only, started through `nsrun-xserver` the way `startx`
starts the server: the session's cookie lands in the root namespace, each
listed name gets its own, an unknown cookie is refused, and a root
namespace client's resources and clipboard are not visible from another
namespace. Not tested on the real server.

## Left out

- A seccomp filter. The program can make any system call the kernel
  offers an unprivileged user.
- Wayland, and X displays that are not local.
- Input methods that run as a separate X client (XIM) do not reach a
  program in an X namespace. kime's GTK and Qt modules run inside the
  program and need nothing from outside.
- The XSETTINGS manager ([scd](../scd)) is another namespace's client too:
  with `-x`, GTK gets the font resolution from `Xft.dpi` but not the window
  scale.

Uses `sh`, `bwrap`, `xauth`, `sed`, `realpath`, `dbus-run-session` if it
is there and, with `-x`, `xrdb`;
`nsrun-xserver` also `mcookie`.
