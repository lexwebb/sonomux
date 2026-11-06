---
name: debug-audio
description: Debug Sonomux audio problems on macOS: an app playing on the wrong channel, routes lost after sleep, the mic indicator flashing or stuck on, garbled or dropping audio in calls, or the daemon hanging. Use when the user reports odd audio behaviour on the Mac, before changing routing code.
---

# Debugging Sonomux audio on macOS

The daemon log rarely tells the whole story. The evidence lives in three places: the
daemon log, the macOS unified log (coreaudiod and ControlCenter), and what the HAL reports
right now. Collect from all three before guessing.

## 1. Triage first

```sh
.claude/skills/debug-audio/triage.sh "2026-11-06 10:50:00"   # or no argument: last 2 hours
```

It prints daemon state, route events, rebuilds per minute, coreaudiod overloads per minute,
mic/audio indicator changes, apps muted by our taps, and sleep/wake. Ask the user roughly
when the problem happened and start a few minutes before that.

How to read it:

| Signal | Meaning |
|---|---|
| No `Routing` line since a `GoXLR disconnected` or a wake | Route died with its hardware and wasn't rebuilt |
| Several `Routing` lines a minute for one group | Route flapping: a liveness check misfires |
| `Default … set` repeating | Daemon fighting an app over the default device |
| `Audio IO Overload` bursts | Dropouts: this is what "garbled/distorted" sounds like |
| `mic:sonomuxd` in indicator changes | Our routes are opening a microphone (they must not) |
| An app's `mic:` toggling every ~50 ms | Its voice processing keeps restarting (call start failing) |

## 2. How routes behave (know this before reading logs)

- Route = process tap (`MutedWhenTapped`) + private aggregate of the target hardware + an
  IOProc. Code: `crates/sonomux-backend/src/coreaudio_tap.rs` and `coreaudio_routing.rs`.
- A tap's IOProc only counts frames while a tapped app is producing sound. Idle apps show
  0 frames: that is normal, not a dead route.
- A route is rebuilt when: its process set changes, its hardware's object ID changes
  (replug/wake), or its apps are playing but its frame counter stopped.
- Route aggregates must only use the tap's input stream (`tap_input_only`). If the
  orange mic dot shows for sonomuxd, that guard is broken.
- macOS has no surround stage: "Surround" routes to the Game channel.

## 3. Check what is live right now

- Menu bar dot next to Control Center: **orange = microphone**, **purple = audio capture**
  (our taps). Screenshot and crop it instead of asking the user:
  `screencapture -x -m out.png && sips -c 40 900 --cropOffset 0 2540 out.png --out crop.png`
  (offset is for a 3440 px wide display; adjust to the screen width).
- HAL processes running audio: `swiftc -O procs.swift -o procs && ./procs`. Caveat: a tap
  counts as input, so `in=1` for sonomuxd does not by itself mean the mic.
- Is Spotify playing? `osascript -e 'tell application "Spotify" to player state'`
- Daemon stuck? `sample $(pgrep -x sonomuxd) 2 | grep -E "AppRoute|SetPropertyData|CreateIOProcID|mach_msg"`

## 4. Reproduce a route without touching the user's apps

Route `afplay` through a channel, play a quiet system sound, then clear the override:

```sh
cp ~/.config/sonomux/config.toml "$SCRATCH/config.before.toml"   # saving rewrites the file
cp .claude/skills/debug-audio/override.rs crates/sonomux-daemon/examples/
cargo build --release -p sonomux-daemon --example override
./target/release/examples/override afplay Music
afplay -v 0.03 -t 5 /System/Library/Sounds/Submarine.aiff &
# ...observe (dot, procs, log)...
./target/release/examples/override afplay ""
rm -rf crates/sonomux-daemon/examples                            # never commit it
```

Don't play music in the user's apps to test; use `afplay` at low volume.

## 5. A/B a fix against the known-good daemon

Build both binaries into the scratchpad, then swap them to compare the same observation:

```sh
cargo build --release -p sonomux-daemon && cp target/release/sonomuxd "$SCRATCH/sonomuxd.new"
git stash && cargo build --release -p sonomux-daemon && cp target/release/sonomuxd "$SCRATCH/sonomuxd.good"; git stash pop

swap() {  # swap <binary>
  launchctl bootout "gui/$(id -u)/org.sonomux.sonomuxd"; sleep 3
  rm -f ~/.local/bin/sonomuxd && cp "$1" ~/.local/bin/sonomuxd
  launchctl bootstrap "gui/$(id -u)" ~/Library/LaunchAgents/org.sonomux.sonomuxd.plist
}
```

Keep `sonomuxd.good` until the fix is merged: it is the rollback.

## Hazards (each one bit us)

- **Never overwrite `~/.local/bin/sonomuxd` in place.** Always `rm` then `cp`. Overwriting
  a binary macOS has cached gives `last exit reason = OS_REASON_CODESIGNING` and launchd
  won't start it (`Bootstrap failed: 5`).
- **coreaudiod can wedge.** Symptom: no routes after start, and `sample` shows the daemon
  in `mach_msg` under `HALC_ProxyObject::SetPropertyData` (e.g. from `AudioDeviceCreateIOProcID`).
  That is coreaudiod, not our code: check with the known-good binary before blaming the
  change. Restart needs admin, and `! sudo` has no terminal for the password, so use:
  `osascript -e 'do shell script "killall coreaudiod" with administrator privileges'`
  (the user types the password in a dialog; all audio drops for ~2 s). Ask first.
- Killing the daemon (or coreaudiod) while it creates a tap can leave coreaudiod wedged.
  Prefer `launchctl bootout` over `pkill -9`.
- In zsh, `log` is a shell builtin: call `/usr/bin/log show`, or you get "too many
  arguments" (and silently empty output if stderr is hidden).
- An `AVAudioEngine` voice-processing test run from the CLI fails with `-10875` even on
  built-in devices. It proves nothing; don't use it as evidence.
- Stopping the daemon unroutes every app (they fall back to System). Say so before doing it,
  keep it short, and confirm `Routing` lines come back afterwards.
- `set_app_override` saves the config, which rewrites the header, reorders keys and writes
  defaults out explicitly. Back the file up first and tell the user.

## Before calling it fixed

- Re-run `triage.sh` from just before your change: no flapping, no `mic:sonomuxd`, routes present.
- If the bug needs a real call or a sleep you can't trigger, say so plainly and ask the user
  to test, then re-run triage over that window.
