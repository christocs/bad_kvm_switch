# AGENTS.md

Instructions for AI coding agents (and future-you) working on this repo.
See [README.md](README.md) for the human-facing pitch, and
[SKILLS.md](SKILLS.md) for step-by-step workflows.

## What this is

A lightweight Rust CLI/background service that watches for a specific USB
peripheral (a shared keyboard/mouse behind a KVM-style USB switch)
connecting, and switches a monitor's DDC/CI input accordingly. Runs as a
separate instance on each PC (Windows + Linux); each instance only reacts
to *its own* peripheral-connect event. See "Architecture" below.

## Build

### Windows

The default Rust host toolchain (`x86_64-pc-windows-msvc`) needs
`link.exe` from Visual Studio Build Tools, which is a multi-GB install.
This repo is set up to use the **GNU toolchain** instead, which needs only
a lightweight MinGW-w64 distribution:

```powershell
rustup target add x86_64-pc-windows-gnu
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
```

Then install a MinGW-w64 distribution providing `gcc`/`dlltool`/`ld`
(rustup's `rust-mingw` component alone is *not* sufficient — it ships
import libs, not the actual linker/binutils):

```powershell
winget install --id BrechtSanders.WinLibs.POSIX.UCRT -e
```

**PATH gotcha**: installing new tools (rustup, winget packages) updates
the registry-persisted PATH, but a shell session started *before* that
install keeps its stale in-memory copy. If `cargo`/`rustc`/a newly
installed tool isn't found, refresh PATH from the registry for that
session rather than assuming a reboot is needed:

```powershell
$env:PATH = [System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' +
            [System.Environment]::GetEnvironmentVariable('Path','User')
```

Then `cargo build` as normal.

### Linux

Needs `libudev-dev` and `pkg-config` (used by `ddc-i2c`'s device
enumeration feature — `ddc-hi` already requires this transitively for its
own Linux DDC support, so this isn't new weight this project adds):

```bash
sudo apt-get install libudev-dev pkg-config   # Debian/Ubuntu
```

then `cargo build`.

`/dev/i2c-*` access requires either `i2c` group membership
(`sudo usermod -aG i2c $USER`, then log out/in) or a udev rule — same
prerequisite `ddcutil` has. Without it, DDC/I2C commands fail with a
permissions error, not a panic.

**Cross-compiling from Windows for a quick Linux type-check doesn't
fully work**: `cargo check --target x86_64-unknown-linux-gnu` gets past
most of the tree but fails on `libudev-sys`'s build script, which needs a
real Linux sysroot for `pkg-config` cross-compilation. This is a real gap
in local verification — the [CI workflow](.github/workflows/build.yml) is
the actual source of truth for "does this compile on Linux," since GitHub's
`ubuntu-latest` runner has `libudev`/`pkg-config` natively.

## Architecture

Plain binary crate, modules cfg-gated per OS where the underlying API is
platform-specific:

- `main.rs` — CLI dispatch, config-driven default run mode.
- `cli.rs` — `clap` derive definitions.
- `config.rs` — `config.toml` loading/validation (`serde` + `toml`,
  `thiserror` for error variants).
- `usb_watch.rs` — `nusb` hotplug watch loop (cross-platform).
- `ddc_control.rs` — standard DDC/CI via `ddc-hi` (cross-platform; used
  for `switch_method = "standard"` and the `ddc-get`/`ddc-set` debug
  commands).
- `adl.rs` (`#[cfg(target_os = "windows")]`) — LG "DDC alt-mode" side
  channel via AMD's legacy ADL SDK, dynamically loaded (`libloading`) —
  see "Known traps" below for why this exists instead of a normal DDC
  call.
- `linux_i2c.rs` (`#[cfg(target_os = "linux")]`) — the same alt-mode side
  channel on Linux, via a raw `i2c_transfer` block write (`ddc-i2c` +
  `i2c-linux`), reusing `ddc-i2c`'s own device enumeration rather than
  re-discovering `/dev/i2c-*` buses independently.
- `service.rs` — per-user background service install/uninstall/status
  (systemd user unit on Linux, a Startup-folder shortcut on Windows).
  Internally cfg-gated per OS behind a shared `platform` module, following
  the same pattern as `main.rs`'s `switch_lg_alt_mode`. Both platforms copy
  the currently-running binary to a stable per-OS data directory
  (`directories::ProjectDirs::data_local_dir()`) before installing, so the
  service doesn't end up pointing at a `target/debug/...` path that a
  later `cargo clean` would break.

`switch_method` in config picks between the standard DDC path and the
alt-mode path; alt-mode is Windows/AMD + Linux only so far (see
`main.rs`'s `switch_lg_alt_mode`).

## Known traps (read before touching `adl.rs`/`linux_i2c.rs`)

- **`libloading::Library::get::<T>()`** must be instantiated with the
  *actual function pointer type* (`T = unsafe extern "C" fn(...) -> ...`),
  not a generic data pointer type like `*const ()` followed by a manual
  `.cast()` + deref. The latter reads the wrong bytes as a function
  address and causes a hard access violation (0xC0000005) — this
  happened once in `adl.rs`'s symbol loading and is exactly why that
  module's `symbol!` macro takes an explicit type parameter.
- **ADL's `AdapterInfo::i_vendor_id`** reports AMD's vendor ID as the
  *decimal digits* `1002`, not the actual hex value `0x1002` (which is
  4098 decimal). Confirmed empirically in `adl.rs`'s `AMD_VENDOR_ID`
  constant — don't "fix" it back to `0x1002`.
- **Standard DDC/CI (`0x60` Input Select) doesn't work at all** on some
  monitors (confirmed on an LG 39GX950B-B: reads succeed, writes are
  silently ignored — ACK'd but no effect, and the value doesn't even
  persist). This is *why* the alt-mode side channel
  (`adl.rs`/`linux_i2c.rs`) exists. It uses DDC/CI source address `0x50`
  instead of the standard `0x51` — neither the Windows Monitor
  Configuration API nor `ddc-i2c`'s normal API can request a non-standard
  source address, hence the raw I2C/ADL implementations.
- **`0x60`'s GET does not track state changed via the alt-mode SET path**
  (confirmed empirically — switched the monitor via `0xF4` alt-mode, then
  read `0x60` immediately after: still reported the old value). Don't
  build a "skip redundant switch" optimization for `lg_alt_mode` based on
  reading `0x60` — it won't reflect reality. (The `standard` switch method
  *does* support this safely; see `ddc_control::set_vcp_if_needed`.)
- **AMD's newer ADLX SDK's `IADLXI2C` does not reach a monitor's DDC
  pins** — it's scoped per-GPU (auxiliary board I2C buses: fan
  controllers, RGB, etc.), not per-display. Every `ADLX_I2C_LINE` reported
  the DDC address as unsupported in testing. The legacy ADL SDK's
  `ADL_Display_DDCBlockAccess_Get` (adapter *and* display scoped) is the
  one that actually works — this was a real dead end during development,
  not an untried option.
- **PowerShell's `$collection -ne $null`** doesn't behave like a plain
  boolean check when `$collection` might be empty (array-comparison
  semantics, not scalar comparison) — `service.rs`'s Windows `status`
  check silently reported "running" when nothing was, using this pattern.
  Use `if ($collection) { ... } else { ... }` (truthy check) instead.
- **Matching a running process by name alone always finds at least
  yourself**, if the checking process shares that name. `service.rs`'s
  Windows `status` command *is* a `bad_kvm_switch.exe` process while it
  runs, so `Get-Process -Name bad_kvm_switch` trivially matched itself.
  Exclude the current PID (`std::process::id()`) and match against the
  *installed* binary's specific path, not just the process name.

## Testing

No unit tests — this is almost entirely I/O against physical hardware
(USB hotplug, a real monitor's DDC/CI channel), which doesn't mock
meaningfully. Verification is manual, via the debug CLI subcommands:

- `list` — enumerate USB devices, find your peripheral's VID:PID.
- `watch` — print connect/disconnect events for the configured device,
  without switching anything (safe to run without touching monitor state).
- `ddc-get`/`ddc-set` — read/write a standard VCP feature directly.
- `ddc-adl-probe`/`ddc-adl-set` (Windows) — list ADL adapter/display
  indices, test the alt-mode switch directly.
- `ddc-linux-alt-set` (Linux) — test the alt-mode switch directly.

Full end-to-end verification requires the real KVM switch and monitor —
run with no subcommand (the default run mode) and physically toggle the
switch. See [SKILLS.md](SKILLS.md) for the exact sequence.

## Conventions

- `anyhow` at call-site boundaries (`main.rs`), `thiserror` for
  domain-specific error enums inside modules (`config::ConfigError`).
- `tracing`/`tracing-subscriber` for operational/lifecycle logging
  (connect/disconnect, switch attempts); plain `println!` for direct
  command *output* (`list`, `ddc-get`, etc.) that's meant to be read or
  piped, not filtered by log level.
- Platform-specific code lives in its own `#[cfg(target_os = "...")]`-gated
  module (`adl.rs`, `linux_i2c.rs`), not scattered `#[cfg]` blocks inside
  shared modules.
- Don't add tests, error handling, or config fields for scenarios that
  can't happen — this is a small, single-purpose tool, not a library.
