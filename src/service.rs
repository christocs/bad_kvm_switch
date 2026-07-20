//! Per-user background service install/uninstall/status.
//!
//! Deliberately per-user, not a Windows Service or systemd *system* unit:
//! both of those need elevated privileges and run outside the interactive
//! session, which breaks USB-notification delivery reliability, and are
//! unnecessary weight for a single-user desktop tool. `display-switch`
//! (the cross-platform reference project this port draws on) takes the
//! same per-user approach.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Stable per-user install location, independent of wherever `cargo
/// build`'s output currently lives -- so the service keeps working after
/// a `cargo clean` or a moved/deleted build directory. Mirrors
/// `betterdisplay-kvm`'s own approach of copying itself into an
/// application-support directory at install time.
fn install_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "bad_kvm_switch")
        .context("could not determine a data directory for this platform")?;
    Ok(dirs.data_local_dir().to_path_buf())
}

fn installed_binary_path() -> Result<PathBuf> {
    let mut path = install_dir()?;
    let file_name = if cfg!(target_os = "windows") {
        "bad_kvm_switch.exe"
    } else {
        "bad_kvm_switch"
    };
    path.push(file_name);
    Ok(path)
}

/// Copy the currently-running executable to the stable install location.
fn copy_self_to_install_dir() -> Result<PathBuf> {
    let current_exe =
        std::env::current_exe().context("failed to determine the current executable's path")?;
    let dest_dir = install_dir()?;
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("failed to create {}", dest_dir.display()))?;
    let dest = installed_binary_path()?;
    // Running `install` from the already-installed binary would copy the
    // file onto itself -- nothing to do in that case.
    if current_exe == dest {
        return Ok(dest);
    }
    std::fs::copy(&current_exe, &dest)
        .with_context(|| format!("failed to copy binary to {}", dest.display()))?;
    Ok(dest)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::process::Command;

    const UNIT_NAME: &str = "bad_kvm_switch.service";

    fn unit_path() -> Result<PathBuf> {
        let base = directories::BaseDirs::new().context("could not determine home directory")?;
        Ok(base
            .home_dir()
            .join(".config/systemd/user")
            .join(UNIT_NAME))
    }

    pub fn install() -> Result<()> {
        let binary = copy_self_to_install_dir()?;
        let unit = unit_path()?;
        std::fs::create_dir_all(unit.parent().expect("unit path has a parent"))?;
        let contents = format!(
            "[Unit]\n\
             Description=bad_kvm_switch - automatic DDC/CI monitor switching\n\
             After=graphical-session.target\n\
             \n\
             [Service]\n\
             ExecStart=\"{}\"\n\
             Restart=on-failure\n\
             RestartSec=2\n\
             \n\
             [Install]\n\
             WantedBy=default.target\n",
            binary.display()
        );
        std::fs::write(&unit, contents)
            .with_context(|| format!("failed to write {}", unit.display()))?;

        run_checked(&["systemctl", "--user", "daemon-reload"])?;
        run_checked(&["systemctl", "--user", "enable", UNIT_NAME])?;
        // `restart` rather than `enable --now`: on a re-install over an
        // already-running unit, `--now` would leave the OLD binary running;
        // restart picks up the freshly-copied one either way.
        run_checked(&["systemctl", "--user", "restart", UNIT_NAME])?;
        println!("Installed and started {UNIT_NAME} (binary copied to {}).", binary.display());
        println!("Check status any time with: systemctl --user status {UNIT_NAME}");
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        // Best-effort: don't fail the whole uninstall if it was already stopped/disabled.
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", UNIT_NAME])
            .status();

        let unit = unit_path()?;
        if unit.exists() {
            std::fs::remove_file(&unit)
                .with_context(|| format!("failed to remove {}", unit.display()))?;
        }
        run_checked(&["systemctl", "--user", "daemon-reload"])?;
        println!("Uninstalled {UNIT_NAME}.");
        Ok(())
    }

    pub fn status() -> Result<()> {
        let unit = unit_path()?;
        if !unit.exists() {
            println!("Not installed (no unit file at {}).", unit.display());
            return Ok(());
        }
        // `systemctl status` exits non-zero for an inactive-but-installed
        // unit, which is a normal state here, not a failure -- don't treat
        // it as one.
        Command::new("systemctl")
            .args(["--user", "status", UNIT_NAME, "--no-pager"])
            .status()
            .context("failed to run systemctl")?;
        Ok(())
    }

    fn run_checked(args: &[&str]) -> Result<()> {
        let status = Command::new(args[0])
            .args(&args[1..])
            .status()
            .with_context(|| format!("failed to run `{}`", args.join(" ")))?;
        if !status.success() {
            anyhow::bail!("`{}` exited with {status}", args.join(" "));
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::process::Command;

    fn startup_shortcut_path() -> Result<PathBuf> {
        let appdata = std::env::var_os("APPDATA")
            .context("%APPDATA% is not set")?;
        Ok(PathBuf::from(appdata)
            .join(r"Microsoft\Windows\Start Menu\Programs\Startup")
            .join("bad_kvm_switch.lnk"))
    }

    /// Windows has no simple native "create a .lnk" call from Rust without
    /// pulling in COM/IShellLink FFI -- shelling out to a one-line
    /// PowerShell script that uses the same WScript.Shell COM object any
    /// GUI "create shortcut" tool uses is far lighter, consistent with
    /// already shelling out to `systemctl` on the Linux side.
    fn run_powershell(script: &str) -> Result<()> {
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .status()
            .context("failed to run powershell")?;
        if !status.success() {
            anyhow::bail!("powershell exited with {status}");
        }
        Ok(())
    }

    /// Stop any running instance of the *installed* binary (matched by
    /// path, excluding this process). Needed before re-install: Windows
    /// won't let `fs::copy` overwrite a running executable, and without a
    /// stop the fresh spawn would run alongside the old instance.
    fn stop_installed_instance() -> Result<()> {
        let installed = installed_binary_path()?;
        let script = format!(
            "Get-Process -Name bad_kvm_switch -ErrorAction SilentlyContinue | \
             Where-Object {{ $_.Id -ne {} -and $_.Path -eq '{}' }} | \
             Stop-Process -Force",
            std::process::id(),
            installed.display()
        );
        run_powershell(&script)
    }

    pub fn install() -> Result<()> {
        stop_installed_instance()?;
        let binary = copy_self_to_install_dir()?;
        let shortcut = startup_shortcut_path()?;
        std::fs::create_dir_all(shortcut.parent().expect("shortcut path has a parent"))?;

        // WindowStyle 7 = minimized -- this is a console-subsystem binary,
        // so a future login-triggered launch via this shortcut still
        // briefly creates a window; minimized keeps it from popping into
        // focus. Fully suppressing the window entirely would need a
        // wscript/.vbs wrapper (WshShortcut has no "hidden" WindowStyle) --
        // more moving parts than seems worth it for a minimized taskbar
        // blip once per login.
        let script = format!(
            "$WshShell = New-Object -ComObject WScript.Shell; \
             $Shortcut = $WshShell.CreateShortcut('{}'); \
             $Shortcut.TargetPath = '{}'; \
             $Shortcut.WorkingDirectory = '{}'; \
             $Shortcut.WindowStyle = 7; \
             $Shortcut.Save()",
            shortcut.display(),
            binary.display(),
            binary.parent().expect("binary path has a parent").display(),
        );
        run_powershell(&script)
            .with_context(|| format!("failed to create shortcut at {}", shortcut.display()))?;

        // Start it now too, for parity with Linux's `enable --now` -- don't
        // wait for the next login to see it working. Unlike the shortcut
        // above, spawning directly lets us fully suppress the console
        // window via CREATE_NO_WINDOW rather than just minimizing it.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new(&binary)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .with_context(|| format!("failed to launch {}", binary.display()))?;

        println!("Installed and started (binary copied to {}).", binary.display());
        println!("Shortcut created at {} -- will auto-start on next login.", shortcut.display());
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        stop_installed_instance()?;
        let shortcut = startup_shortcut_path()?;
        if shortcut.exists() {
            std::fs::remove_file(&shortcut)
                .with_context(|| format!("failed to remove {}", shortcut.display()))?;
            println!("Removed startup shortcut at {} and stopped the running instance.", shortcut.display());
        } else {
            println!("Not installed (no shortcut at {}).", shortcut.display());
        }
        Ok(())
    }

    pub fn status() -> Result<()> {
        let shortcut = startup_shortcut_path()?;
        if shortcut.exists() {
            println!("Installed: startup shortcut present at {}.", shortcut.display());
        } else {
            println!("Not installed (no shortcut at {}).", shortcut.display());
        }

        // Two bugs caught testing this live: (1) `-ne $null` on a possibly-
        // empty collection is a classic PowerShell trap (array-comparison
        // semantics, not a plain boolean check) -- silently reports
        // "running" when nothing is; the truthy `if (...)` form is the
        // reliable pattern. (2) matching by process name alone always
        // finds at least the `status` invocation itself (it IS a
        // bad_kvm_switch.exe process, alive for the duration of this
        // check) -- exclude our own PID, and match specifically against
        // the installed binary's path so a manually-run debug build
        // doesn't get mistaken for the installed service either.
        let installed_path = installed_binary_path()?;
        let current_pid = std::process::id();
        let script = format!(
            "$p = Get-Process -Name bad_kvm_switch -ErrorAction SilentlyContinue | \
             Where-Object {{ $_.Id -ne {current_pid} -and $_.Path -eq '{}' }}; \
             if ($p) {{ 'True' }} else {{ 'False' }}",
            installed_path.display()
        );
        let running = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .context("failed to query running processes")?;
        let is_running = String::from_utf8_lossy(&running.stdout).trim() == "True";
        println!("Currently running: {is_running}");
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    use super::*;

    pub fn install() -> Result<()> {
        anyhow::bail!("service install is only implemented for Linux and Windows so far")
    }
    pub fn uninstall() -> Result<()> {
        anyhow::bail!("service uninstall is only implemented for Linux and Windows so far")
    }
    pub fn status() -> Result<()> {
        anyhow::bail!("service status is only implemented for Linux and Windows so far")
    }
}

pub fn install() -> Result<()> {
    platform::install()
}

pub fn uninstall() -> Result<()> {
    platform::uninstall()
}

pub fn status() -> Result<()> {
    platform::status()
}
