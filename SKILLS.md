# SKILLS.md

Step-by-step workflows for setting up, testing, and extending this project.
See [AGENTS.md](AGENTS.md) for build instructions and architecture, and
[README.md](README.md) for the overview.

## First time opening this repo in VSCode

[.vscode/extensions.json](.vscode/extensions.json) lists recommended
extensions (`rust-analyzer`, TOML support, a Rust-capable debugger,
dependency-version hints, GitHub Actions support) — VSCode shows an
"Install All" prompt automatically when you open the folder, if you don't
already have them. [.vscode/launch.json](.vscode/launch.json) has a debug
configuration (breakpoints, step-through via CodeLLDB) for every CLI
subcommand, so you can step through the `unsafe` FFI code in
`adl.rs`/`linux_i2c.rs` instead of `println!`-debugging it — open the "Run
and Debug" panel and pick the command you want to step through.

## Find your peripheral's VID:PID

```sh
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

```sh
cargo run -- ddc-adl-probe
```

Lists every connected display as `adapter=<N> display=<N> <name>`. If your
monitor appears under multiple adapter indices (common — ADL enumerates
one logical adapter per output/mode combination), any one of them that's
listed should work; there's no need to test all of them. Then:

```sh
cargo run -- ddc-adl-set 0xf4 0xd0 --adapter <N> --display <N>
```

**Linux**:

```sh
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

```sh
cargo run -- watch
```

Prints `connected`/`disconnected` as the configured device toggles, using
whatever `usb_device_id` is in your config. Doesn't touch the monitor —
safe to leave running while you toggle the physical switch to confirm
detection timing/correctness before testing the real switch behavior.

## Test the full end-to-end loop

```sh
cargo run
```

(No subcommand = the real run mode.) To see a *visible* switch (not just a
no-op if the monitor's already on the right input), manually flip the
monitor to a different input via its OSD first, then toggle the physical
KVM switch away and back — it should auto-switch back within a couple of
seconds.

## Install as a background service

```sh
cargo run -- install     # copies the binary to a stable location, sets up
                          # auto-start, and starts it immediately
cargo run -- status       # check whether it's installed/running
cargo run -- uninstall    # remove it
```

Per-user only on both platforms (no admin/root, no system-wide service) —
Linux gets a `systemd --user` unit (`~/.config/systemd/user/bad_kvm_switch.service`,
`systemctl --user status bad_kvm_switch` to check directly), Windows gets a
Startup-folder shortcut (`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`).
Both copy the currently-running binary to a stable per-OS data directory
first, so the service keeps working after a `cargo clean` or a deleted
build directory — it's not pointing at `target/debug/...`.

`install` starts the service immediately (Linux: `systemctl --user enable`
then `restart`; Windows: spawns it directly with a suppressed console
window) — don't run it unless you're ready for it to actually start switching your
monitor on future USB events, same caution as running the real end-to-end
loop above. Re-running `install` after code changes is the intended
upgrade path: it stops the old running instance, overwrites the installed
binary, and starts the new one. `uninstall` also stops the running
instance, not just the autostart entry.

The running service writes rotating file logs (daily, 7 kept) to
`%LOCALAPPDATA%\bad_kvm_switch\data\logs` (Windows) /
`~/.local/share/bad_kvm_switch/logs` (Linux) — the first place to look
when the installed service doesn't behave, since it has no visible
console. On Linux `journalctl --user -u bad_kvm_switch` also has the
console stream.

Two real bugs were caught testing the Windows `status` check live, worth
knowing about if you're touching `service.rs`: PowerShell's `-ne $null` on
a possibly-empty collection doesn't behave like a plain boolean check (use
`if (Get-Process ...) { ... }` instead), and matching by process name
alone always finds at least the `status` command's own process (it *is* a
`bad_kvm_switch.exe`) — the fix excludes the current PID and matches
specifically against the installed binary's path.

## Verify both platforms build

Push to a branch with a PR, or check
[.github/workflows/build.yml](.github/workflows/build.yml) runs on
`windows-latest`/`ubuntu-latest`. Local cross-checking from Windows for
Linux doesn't fully work (see AGENTS.md's build section) — CI is the real
verification for the platform you're not currently on.

## Set up MCP servers (for agents working on this repo)

[.mcp.json](.mcp.json) declares three project-shared MCP servers — they'll
be offered to anyone (or any agent) working in this repo, pending a
one-time per-person approval prompt:

- **`playwright`** — browser automation for reading JS-rendered docs sites
  (needed historically for AMD's GPUOpen documentation, which plain HTML
  fetches can't render). No setup beyond Node/`npx` being available.
- **`rust-docs`** — structured Rust crate documentation lookups (signatures,
  types, trait impls) instead of scraping docs.rs by hand. Needs the binary
  installed first: `cargo install docsrs-mcp`. No API key or credentials —
  it just fetches from docs.rs over plain HTTP.
- **`github`** — repo browsing, issues, PRs, code search. Auth is handled
  via the **GitHub CLI** (`gh`), not a raw token in a dotfile — see below.

### One-command setup

```sh
./scripts/setup-mcp.ps1    # Windows
./scripts/setup-mcp.sh     # Linux
```

Installs `docsrs-mcp` (`cargo install`) and the GitHub CLI if missing, then
tells you whether `gh` still needs authenticating. Safe to re-run — every
step checks before installing. It can't complete GitHub auth for you
(needs your browser), so if it prints a `gh auth login` reminder, run that
yourself, then re-run the script (or just `gh auth token`) to confirm it
took.

`.mcp.json`'s `github` entry uses a `headersHelper` that calls
`gh auth token` fresh on every connection, so a plaintext token never sits
in an env var or config file — it lives only in `gh`'s own OS-keychain-backed
storage.

After the script finishes: restart VSCode / your Claude Code session, then
approve the MCP servers the first time each is offered (`/mcp` to check
status).

**Windows-specific note**: the `headersHelper` command uses bash-style
quoting (`echo '{"..."}'` with command substitution), which only works
correctly through a POSIX-compatible shell — confirmed it produces broken
JSON if run through plain PowerShell (the substitution gets wrapped in
extra newlines). This works because Claude Code's Windows tooling already
relies on **Git Bash** (bundled with [Git for Windows](https://git-scm.com/),
which you need installed anyway to work with this repo) rather than
PowerShell/cmd.exe for shell commands like this one. If `gh` isn't on the
PATH that Git Bash sees, or Git Bash itself isn't installed, the helper
will fail — install Git for Windows normally (default settings add both
Git and its bundled Bash to PATH) and this should just work.

## Continue development (milestone pattern)

This project was built in small, independently-testable milestones (CLI
skeleton → DDC discovery → USB watch loop → wire them together → config →
service install → polish). If picking up new work, follow the same shape:
one runnable/testable increment at a time, with a concrete manual test
criterion (a CLI subcommand's output, or a physical action + expected
result) — not a batch of changes that only becomes testable at the end.
