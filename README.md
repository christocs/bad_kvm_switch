# bad_kvm_switch

Automatically switch a monitor's input via DDC/CI when a shared USB
keyboard/mouse moves between two PCs on a KVM-style USB switch — no more
reaching for the monitor's OSD every time you press the switch button.

Inspired by [betterdisplay-kvm](https://crates.io/crates/betterdisplay-kvm),
which does the same thing on macOS by riding on top of the BetterDisplay
app. This is a from-scratch Rust port for **Windows and Linux**, written
as a learning project (see [AGENTS.md](AGENTS.md) if you're a coding
agent, or [SKILLS.md](SKILLS.md) for step-by-step setup workflows).

## How it works

```
        ┌───────────────────────────┐              ┌───────────────────────────┐
        │   bad_kvm_switch (Linux)   │              │  bad_kvm_switch (Windows)  │
        │                            │              │                            │
 USB ──▶│ watches for peripheral     │              │ watches for peripheral     │◀── USB
switch  │   connecting                │              │   connecting                │  switch
        │        │                  │              │        │                  │
        │        ▼                  │              │        ▼                  │
        │ sends DDC/CI switch        │              │ sends DDC/CI switch        │
        │   command to monitor       │              │   command to monitor       │
        └────────────┼───────────────┘              └────────────┼───────────────┘
                      ▼                                            ▼
              Monitor DDC/CI channel                       Monitor DDC/CI channel
              (this PC's own video cable)                  (this PC's own video cable)
```

The same binary runs independently on **both** PCs. Each instance only
watches for *its own* peripheral to connect (meaning the physical switch
just pointed at it) and tells the monitor to switch to itself — it never
needs to act on a disconnect, since the other machine's own connect event
handles switching away. This means it keeps working even if one PC is
powered off.

## Status

**Windows**: fully live-tested end-to-end — USB hotplug detection → DDC/CI
switch (both the standard MCCS path and the vendor-specific "alt mode"
side channel, see below) → per-user background service install, all
confirmed working against real hardware.

**Linux**: the same alt-mode side channel is implemented (`src/linux_i2c.rs`,
raw I2C block write) and the systemd user-service install path is written,
but neither has been run on real Linux hardware yet — verification is
pending. Local cross-compilation from Windows can't fully check this code
(see AGENTS.md), so CI and an actual Linux test are the real verification.

Not yet done: retry/backoff around transient DDC failures, graceful
`Ctrl+C` shutdown. See the todo list in-repo for the current milestone.

## Two ways to switch the input

Most monitors support the standard DDC/CI "Input Select" command
(`switch_method = "standard"`) — plain, cross-platform, and reliable.

Some monitors (confirmed on an LG 45GX950A/UltraGear) silently ignore that
command entirely — DDC reads work, but writes to the input-select feature
are ACK'd and then dropped on the floor. These monitors have a
manufacturer-specific "alt mode" side channel instead (LG's uses DDC/CI
source address `0x50` in place of the standard `0x51`, which neither
platform's normal DDC API can request). `switch_method = "lg_alt_mode"`
implements that side channel directly: via AMD's ADL SDK on Windows
(`src/adl.rs`), and via a raw I2C block write on Linux (`src/linux_i2c.rs`).
See [AGENTS.md](AGENTS.md) for the full story of how that was figured out
— it involved a wrong turn through AMD's newer ADLX SDK before landing on
the legacy ADL API that actually works.

## Build

```
cargo build
```

Windows needs the GNU Rust toolchain (not the MSVC default) plus a
lightweight MinGW-w64 install; Linux needs `libudev-dev`/`pkg-config`. See
[AGENTS.md](AGENTS.md) for exact commands and why.

## Configure

Create `config.toml` at the platform default location (Linux:
`~/.config/bad_kvm_switch/config.toml`; Windows:
`%APPDATA%\bad_kvm_switch\config\config.toml`; run once with no config
present and the error tells you the exact path), or point at one with
`--config <path>`:

```toml
# VID:PID of the shared keyboard/mouse, watched for a CONNECT event on
# THIS machine. Format: "vvvv:pppp" lowercase hex. Find yours with
# `bad_kvm_switch list`.
usb_device_id = "8968:4e4b"

# "standard" (most monitors) or "lg_alt_mode" (see above). Figure out
# which you need per SKILLS.md.
switch_method = "lg_alt_mode"

# VCP feature code to write, as hex. 0x60 for "standard"; LG alt-mode
# monitors use 0xF4.
vcp_feature = "0xF4"

# Raw value written to vcp_feature to select THIS pc's input.
# Standard 0x60 mode: 0x0f=DisplayPort-1, 0x11=HDMI-1, etc. -- confirm via
#   `bad_kvm_switch ddc-get 0x60`.
# LG alt mode (0xF4): hdmi1=0x90, hdmi2=0x91, dp=0xD0, usbc=0xD1.
input_value = "0xD0"

# Only used when switch_method = "lg_alt_mode" on Windows (via ADL).
# Find yours with `bad_kvm_switch ddc-adl-probe`.
adl_adapter = 5
adl_display = 0

# trace | debug | info | warn | error
log_level = "info"
```

Then run it:

```
cargo run
```

Or install it as a per-user background service (auto-starts on login, no
admin/root needed):

```
cargo run -- install
```

See [SKILLS.md](SKILLS.md) for how to find each config value for your own
hardware, the debug subcommands (`list`, `watch`, `ddc-get`,
`ddc-adl-probe`, etc.) that let you test each piece in isolation, and more
on `install`/`status`/`uninstall`.

## Credits

- [betterdisplay-kvm](https://crates.io/crates/betterdisplay-kvm) — the
  macOS original this project ports the concept from.
- [haimgel/display-switch](https://github.com/haimgel/display-switch) —
  proof that the `arcnmx` DDC crate family (`ddc`, `ddc-hi`, `ddc-i2c`,
  `ddc-winapi`) works well for exactly this use case, cross-platform.
- [ddcutil's wiki on LG alt-mode switching](https://github.com/rockowitz/ddcutil/wiki/Switching-input-source-on-LG-monitors),
  [phillip9933/LGInputSwitch](https://github.com/phillip9933/LGInputSwitch) (MIT),
  and [amildahl/amdddc-windows](https://github.com/amildahl/amdddc-windows) —
  documented and demonstrated the LG alt-mode protocol and the AMD ADL call
  sequence that actually reaches it.

## License

MIT — see [LICENSE](LICENSE).
