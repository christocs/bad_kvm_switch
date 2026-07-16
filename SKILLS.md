# SKILLS.md

Step-by-step workflows for setting up, testing, and extending this project.
See [AGENTS.md](AGENTS.md) for build instructions and architecture, and
[README.md](README.md) for the overview.

## Find your peripheral's VID:PID

```
cargo run -- list
```

Prints every attached USB device as `vvvv:pppp<TAB>manufacturer<TAB>product<TAB>serial`.
Since the target peripheral goes through a KVM switch, cross-check by
running `cargo run -- watch` (see below) and physically pressing the
switch — the VID:PID that logs a `connected`/`disconnected` pair in sync
with the button press is the right one, not just whatever looks plausible
by name.

## Determine whether you need `standard` or `lg_alt_mode`

1. Try the standard path first: `cargo run -- ddc-get 0x60` to read the
   current input, then `cargo run -- ddc-set 0x60 <a-different-known-code>`
   (e.g. `0x11` for HDMI-1, `0x0f` for DisplayPort-1) and watch the
   monitor. If it switches, use `switch_method = "standard"` — you're
   done, no alt-mode setup needed.
2. If nothing happens (common on some LG panels, confirmed on a 45GX950A),
   you need `switch_method = "lg_alt_mode"`. Continue below.

## Find your monitor's alt-mode input codes

LG's alt-mode (`0xF4`) uses its own value encoding, not the standard MCCS
codes: known values are `hdmi1=0x90, hdmi2=0x91, dp=0xD0, usbc=0xD1`. These
appear to be consistent across affected LG models, but confirm by testing
directly (next section) rather than assuming.

## Test the alt-mode switch directly (before wiring up config)

**Windows**: first find your ADL adapter/display indices —

```
cargo run -- ddc-adl-probe
```

Lists every connected display as `adapter=<N> display=<N> <name>`. If your
monitor appears under multiple adapter indices (common — ADL enumerates
one logical adapter per output/mode combination), any one of them that's
listed should work; there's no need to test all of them. Then:

```
cargo run -- ddc-adl-set 0xf4 0xd0 --adapter <N> --display <N>
```

**Linux**:

```
cargo run -- ddc-linux-alt-set 0xf4 0xd0
```

No adapter/display selection needed — it uses the first DDC/CI-capable
I2C device found (this project doesn't yet support multi-monitor
selection; see `ddc_control::first_display`'s doc comment for where that
would go).

Both commands write directly and print success/failure — no physical
switch interaction needed to test this piece in isolation. **Careful**:
this genuinely changes your monitor's input if it works, which can cut off
your view of whatever you're doing on that PC if you switch away from it.

## Write `config.toml`

Default path: `~/.config/bad_kvm_switch/config.toml` (Linux),
`%APPDATA%\bad_kvm_switch\config\config.toml` (Windows). Override with
`--config <path>`. Run once with no config present — the error message
tells you the exact expected path. See [README.md](README.md) for the
field reference and an example.

## Test the watch loop without switching anything

```
cargo run -- watch
```

Prints `connected`/`disconnected` as the configured device toggles, using
whatever `usb_device_id` is in your config. Doesn't touch the monitor —
safe to leave running while you toggle the physical switch to confirm
detection timing/correctness before testing the real switch behavior.

## Test the full end-to-end loop

```
cargo run
```

(No subcommand = the real run mode.) To see a *visible* switch (not just a
no-op if the monitor's already on the right input), manually flip the
monitor to a different input via its OSD first, then toggle the physical
KVM switch away and back — it should auto-switch back within a couple of
seconds.

## Verify both platforms build

Push to a branch with a PR, or check
[.github/workflows/build.yml](.github/workflows/build.yml) runs on
`windows-latest`/`ubuntu-latest`. Local cross-checking from Windows for
Linux doesn't fully work (see AGENTS.md's build section) — CI is the real
verification for the platform you're not currently on.

## Continue development (milestone pattern)

This project was built in small, independently-testable milestones (CLI
skeleton → DDC discovery → USB watch loop → wire them together → config →
service install → polish). If picking up new work, follow the same shape:
one runnable/testable increment at a time, with a concrete manual test
criterion (a CLI subcommand's output, or a physical action + expected
result) — not a batch of changes that only becomes testable at the end.
