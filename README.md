# iHeadphone

A native Linux and macOS desktop controller for AirPods and Sony headphones.

The desktop app uses GPUI Kit. Sony WH-1000XM5 discovery and controls use a local Bluetooth RFCOMM
transport and a reviewed, I/O-free Sony V2 protocol crate. Initial wire layouts and captured test
messages were adapted from the MIT-licensed [`sound-connect`](https://github.com/aecca/sound-connect)
project; its license notice is preserved beside the protocol source.

## Current scope

- Linux: AirPods and Sony WH-1000XM5 control.
- macOS: Sony WH-1000XM5 control. AirPods remain managed by macOS.

## Run with real Sony hardware

```sh
cargo run -p iheadphone-desktop
```

Pair and connect the WH-1000XM5 in the operating system's Bluetooth settings. The app then opens
Sony's control channel and keeps reconnecting when the headphones become available.

The XM5 hardware backend currently supports battery and charging state, ANC/Ambient/Off, ambient
level and Focus on Voice, DSEE Extreme, Speak-to-Chat and its options, five-band EQ with Clear Bass
and presets, firmware and codec readback, wear-sensor pause, automatic power off, the NC/Ambient
button cycle, multipoint device readback and source switching, and software power off.

On Linux, iHeadphone starts BlueZ's `mpris-proxy` while the app is running when its user service is
not already active. This forwards AVRCP media-button commands from the headset to MPRIS players
such as browsers and desktop music apps. Install the BlueZ tools package that provides
`mpris-proxy` if your distribution separates it from BlueZ.

The XM5 does not always emit an AVRCP pause when Speak-to-Chat activates. iHeadphone also listens
for Sony's private active/inactive conversation notifications and pauses the currently playing
MPRIS players with `playerctl`, resuming only the players it paused when the conversation ends.

For a reversible real-hardware write-path check:

```sh
cargo run -p iheadphone-transport --example xm5_smoke
```

## Run the UI with simulated devices

```sh
IHEADPHONE_DEMO=1 cargo run -p iheadphone-desktop
```

## Language

Open **Settings** from the bottom of the sidebar and choose **English** or **简体中文**. The whole
interface switches immediately and remembers the selection across restarts. On Linux the preference
is stored under `$XDG_CONFIG_HOME/iheadphone` (or `~/.config/iheadphone`); on macOS it is stored in
`~/Library/Application Support/iHeadphone`.

For UI development, set `IHEADPHONE_OPEN_SETTINGS=1` to open directly on the settings page.

## Verify

```sh
cargo fmt --check
cargo test --workspace
cargo check --workspace
```

### Wayland desktop icon

Wayland compositors resolve the window icon through the application's desktop ID rather than the
X11 window-icon field. Install the development desktop entry and hicolor icon for the current user:

```bash
sh packaging/linux/install-desktop-integration.sh
```

The application ID and desktop filename are both `io.github.iheadphone.iheadphone`.
