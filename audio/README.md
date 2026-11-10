# audio

Puts the sound system ahead of everything else on the CPU. PipeWire, its
PulseAudio server and the speaker and microphone DSP that asahi-audio runs
inside WirePlumber get real-time priorities above every other program and
every kernel interrupt thread, so that a build, a busy browser or any other
load cannot make the sound skip.

It also keeps the speaker DSP running when playback stops and puts
PipeWire's `zeroramp` in front of it, against the pop the speakers made
whenever playback started or stopped.

| File | What it is |
|---|---|
| [install.sh](install.sh) | Adds the desktop user to the `pipewire` group and installs the rest |
| [pipewire.conf.d/99-wmutils-audio.conf](pipewire.conf.d/99-wmutils-audio.conf) | The PipeWire server's priorities, and the speaker DSP kept running |
| [pipewire-pulse.conf.d/99-wmutils-audio.conf](pipewire-pulse.conf.d/99-wmutils-audio.conf) | Priorities of pipewire-pulse |
| [wireplumber.conf.d/99-wmutils-audio.conf](wireplumber.conf.d/99-wmutils-audio.conf) | The speaker DSP with zeroramp in front |
| [zeroramp.py](zeroramp.py) | Writes a copy of asahi-audio's speaker DSP graph with zeroramp in front |
| [asahi-audio.bashrc](asahi-audio.bashrc) | Portage hook that writes the copy again when asahi-audio is updated |

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

`install.sh` copies the two PipeWire drop-ins to
`/etc/pipewire/pipewire.conf.d/` and `/etc/pipewire/pipewire-pulse.conf.d/`.
They set `module.rt.args`, which overrides the arguments of every
`module-rt` in that process: the one in pipewire.conf (or
pipewire-pulse.conf) and the one asahi-audio's 99-asahi.conf adds.

- `rt.prio` 88 for the server and 83 for pipewire-pulse: PipeWire's own
  defaults, set so that both `module-rt` instances agree. Since nothing
  else runs above 50, higher numbers would change nothing.
- `nice.level` -19 instead of PipeWire's -11, the lowest the group allows:
  the main threads set up streams, devices and links, so a stream still
  starts at once under full load.

WirePlumber's priorities need no drop-in: its configuration asks for nice
-11, `module-rt`'s default priority for clients is 83, and the group now
grants both. `module.rt.args` would not reach it anyway: WirePlumber reads
its own configuration, not PipeWire's.

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

## The pop when playback starts and stops

The speakers popped whenever playback started or stopped, even for a pause
of less than a second. Muting never popped. The pop came from the speaker
DSP, not from the kernel or the amplifiers:

- The DSP is asahi-audio's filter-chain inside WirePlumber, from the
  "MacBook Pro J314 Speakers" sink to the hidden RawSpeakers device. About
  26 ms of sound is inside it at any time.
- PipeWire pauses a node when it goes idle (`node.pause-on-idle` is true
  by default). When playback stops, both DSP nodes go idle, and when its
  output is paused filter-chain drops what is queued and resets the DSP
  (`playback_state_changed` in `module-filter-chain.c`). The sound still
  inside is cut off mid-wave, and the next start goes through a freshly
  reset DSP.
- Browsers cork their stream as soon as playback stops, so even a short
  pause pauses the DSP. Muting leaves it running.

Tested with a 200 Hz tone, cut off and with 20 ms fades, while logging the
PCM state every 5 ms:

| Test | Heard |
|---|---|
| Silence: the speakers open and close, amplifiers on and off | nothing |
| Tones, the speakers open throughout, no restart at any stop | a pop at every start and stop, fades too |
| Tones, with a silent stream keeping the DSP running | the faded tones silent |

The fades did not help because the 26 ms cut off is longer than the fade.

The PipeWire drop-in sets `node.pause-on-idle = false` on both DSP nodes
through `node.rules`, which the server applies over the properties
WirePlumber gives them. asahi-audio's graph.json, where those come from,
takes no drop-ins. Idle, both nodes keep running on silence and the sound
drains out. WirePlumber still suspends the speakers 5 s after playback
stops; the DSP running for those 5 s costs about 1.5 % of one core.
Within those 5 s, playback starts on a DSP that never stopped.

Now and then the ALSA buffer runs dry just after a start (an XRUN) and the
device restarts, which can click too. That is not covered; the cause is not
known yet.

PipeWire 1.6.10 and 1.6.11 change nothing here, and `node.pause-on-idle`
is still true by default on PipeWire's master branch.

## zeroramp

With the DSP kept running, a program that stops its sound mid-wave, as a
browser does when paused, still makes the speakers click: PipeWire plays
exact zeros from the next cycle on, and the sound jumps from wherever the
wave was to zero. It starts the same way, from zero to the middle of a
wave. PipeWire's `zeroramp` filter turns these jumps into short fades. It
passes the sound through until it has seen 0.666 ms of exact zeros (31
samples at 48 kHz), turns those zeros into a fade from the last sample down
to zero, and fades the sound in over the same time when it comes back. It
adds no latency: it only rewrites zeros it has already seen in the same
cycle.

asahi-audio's graph has no zeroramp and takes no drop-ins, so:

- `install.sh` writes a copy of it with a `zeroramp` in front of each of
  its two inputs, ahead of everything else in the DSP, to
  `/usr/local/share/wmutils/audio/j314-graph.json`
  ([zeroramp.py](zeroramp.py)).
- The WirePlumber drop-in has the speakers' DSP run from the copy. Another
  rule for the speakers would start a second DSP, so the drop-in replaces
  asahi-audio's rules with the J314's two: the speakers with the copy, the
  microphones as before. That is why `install.sh` stops on any other Mac.
- An asahi-audio update may change the graph and the filter files it names,
  so `install.sh` also installs a Portage hook,
  `/etc/portage/env/media-libs/asahi-audio`, which writes the copy again
  each time asahi-audio is installed. PipeWire uses it from its next start.
  If zeroramp cannot be added then, the copy is asahi-audio's graph as it
  is, and emerge says so.

The gap and the fade are PipeWire's defaults and have to stay equal: the
fade is written over the zeros counted for the gap, so a shorter fade
leaves zeros in front of it and a longer one overwrites the sound. And the
gap has to fit into PipeWire's shortest cycle, 32 samples, since zeroramp
reaches back only within the cycle it is in. Pausing starts the zeros at
the start of a cycle; where a program stops its sound in the last 31
samples of a cycle instead, the jump stays.

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

If the user was in the group already, as when only the drop-ins change,
restarting PipeWire is enough (`install.sh` says which). `setsid -f` keeps
it running after the terminal closes:

```sh
setsid -f gentoo-pipewire-launcher restart
```

To check the speaker DSP:

```sh
for n in audio_effect effect_output; do pw-cli info $n.j314-convolver 2>/dev/null | grep pause-on-idle; done
```

Both lines show `node.pause-on-idle = "false"`.

To check zeroramp:

```sh
pw-cli enum-params audio_effect.j314-convolver Props 2>/dev/null | grep -A1 zeroramp
```

It shows `Gap (s)` and `Duration (s)` of `zeroramp_FL` and `zeroramp_FR`,
each 0.000666.

The group's limits apply to every program the user runs, as on most
distributions: any of them may now ask for real-time priority.

`sudo sh audio/install.sh remove` undoes it: it removes the drop-ins, the
copy of the graph and the Portage hook, and takes the user out of the
group. After the next login PipeWire uses RTKit again, and asahi-audio's
speaker DSP as it is.
