# audio

Puts the sound system ahead of everything else on the CPU. PipeWire, its
PulseAudio server and the speaker and microphone DSP that asahi-audio runs
inside WirePlumber get real-time priorities above every other program and
every kernel interrupt thread, so that a build, a busy browser or any other
load cannot make the sound skip.

| File | What it is |
|---|---|
| [install.sh](install.sh) | Adds the desktop user to the `pipewire` group and installs the two drop-ins |
| [pipewire.conf.d/99-utils-mydesktop-audio.conf](pipewire.conf.d/99-utils-mydesktop-audio.conf) | Priorities of the PipeWire server |
| [pipewire-pulse.conf.d/99-utils-mydesktop-audio.conf](pipewire-pulse.conf.d/99-utils-mydesktop-audio.conf) | Priorities of pipewire-pulse |

Written for PipeWire 1.6.9, WirePlumber 0.5.18 and asahi-audio 5.0 on Gentoo
with s6 and elogind, PipeWire started by `gentoo-pipewire-launcher` from
`~/.xinitrc`.

## Priorities

Before (measured on an M1 Pro) and after:

| Thread | Before | After |
|---|---|---|
| `pipewire` data loop: the graph's clock and the ALSA devices | RR 20 | FIFO 88 |
| `pipewire-pulse` data loop: browsers, Electron apps and other PulseAudio clients | RR 20 | FIFO 83 |
| `wireplumber` data loops: the speaker DSP (`j314-convolver`) and the microphone DSP | normal, nice 0 | FIFO 83 |
| Data threads of native PipeWire clients | RR 20 at best | FIFO 83 |
| Main threads of `pipewire` and `pipewire-pulse` | nice -11 | nice -19 |
| Main thread of `wireplumber` | nice 0 | nice -11 |
| Kernel interrupt threads: Wi-Fi, keyboard and trackpad, SD card, ... | FIFO 50 | FIFO 50 |
| Every other program | normal | normal |

A FIFO thread runs before every normal thread and every real-time thread
of lower priority, and keeps the CPU until it waits again. Nothing else on
the system runs above 50.

## Why it was not so already

The user was not in the `pipewire` group. PipeWire installs
`/etc/security/limits.d/25-pw-rlimits.conf`, which lets that group use
real-time priorities up to 95, nice levels down to -19 and 4 GiB of locked
memory. Gentoo asks to add desktop users to the group, but says so only
when upgrading from PipeWire 0.3.66 or older. Without the limits (all 0),
PipeWire's `module-rt` asks a D-Bus service instead:

- `pipewire` and `pipewire-pulse` ask RTKit, which gives at most priority
  20 and nice -15: below the kernel's interrupt threads at 50.
- WirePlumber asks xdg-desktop-portal first, because only pipewire.conf and
  pipewire-pulse.conf turn the portal off, and got nothing at all. PipeWire
  clients ask the same way. That left the speaker DSP, which asahi-audio
  runs inside WirePlumber, at normal priority, sharing the CPU equally with
  every compiler of a build.

In the group, every `module-rt` sets its threads' priorities itself, with
no D-Bus service in between.

## The drop-ins

`install.sh` copies them to `/etc/pipewire/pipewire.conf.d/` and
`/etc/pipewire/pipewire-pulse.conf.d/`. They set `module.rt.args`, which
overrides the arguments of every `module-rt` in that process: the one in
pipewire.conf (or pipewire-pulse.conf) and the one asahi-audio's
99-asahi.conf adds.

- `rt.prio` 88 for the server and 83 for pipewire-pulse: PipeWire's own
  defaults, set so that both `module-rt` instances agree. Since nothing
  else runs above 50, higher numbers would change nothing.
- `nice.level` -19 instead of PipeWire's -11, the lowest the group allows:
  the main threads set up streams, devices and links, so a stream still
  starts at once under full load.

WirePlumber needs no drop-in: its own configuration asks for priority 83
and nice -11, which the group now grants. It could not take one anyway: it
reads its own configuration, not PipeWire's, so `module.rt.args` does not
reach it.

## Left as they were

- The interrupts of the DMA controllers that move the audio (ADMAC) are
  hard interrupts. They run before any thread, so there is nothing to
  raise.
- speakersafetyd runs at normal priority. Its V/I sense capture buffer
  holds 131072 frames, 2.7 s at 48 kHz, far longer than a normal thread
  ever waits for the CPU.
- asahi-audio's `uclamp.max = 128`, which keeps WirePlumber and the
  PipeWire main threads at low clocks, on efficiency cores, to save
  battery. It limits clock speed, not priority: the DSP still runs before
  everything else. The data loops of `pipewire` and `pipewire-pulse` are
  not clamped.
- The kernel (`PREEMPT_LAZY`): a real-time thread that wakes up preempts
  at once.

## Setting it up

```sh
sudo sh audio/install.sh
```

then quit Window Maker and start X again from the new login (on tty1 that
is `startx`, since `~/.bash_profile` starts X by itself only once a boot),
or reboot. The limits are set at login (pam_limits), and PipeWire starts
with the new session. To check:

```sh
ps -Lo pid,cls,rtprio,ni,comm -p "$(pgrep -d, -x 'pipewire|wireplumber')"
```

The `data-loop.0` threads show `FF` with 88 or 83, the main threads nice
-19 (`pipewire`) and -11 (`wireplumber`), and the `module-rt` threads, which
only talk to RTKit, are gone.

The group's limits apply to every program the user runs, as on most
distributions: any of them may now ask for real-time priority.

`sudo sh audio/install.sh remove` undoes it: it removes the drop-ins and
takes the user out of the group. After the next login PipeWire uses RTKit
again.
