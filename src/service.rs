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

    /// Registered under this name in the *user's* Task Scheduler library.
    const TASK_NAME: &str = "bad_kvm_switch";

    /// Where pre-Task-Scheduler installs put their launcher. Still cleaned
    /// up by `install`/`uninstall`, so upgrading can't leave a second,
    /// unsupervised copy starting at logon alongside the task.
    fn legacy_startup_shortcut_path() -> Result<PathBuf> {
        let appdata = std::env::var_os("APPDATA").context("%APPDATA% is not set")?;
        Ok(PathBuf::from(appdata)
            .join(r"Microsoft\Windows\Start Menu\Programs\Startup")
            .join("bad_kvm_switch.lnk"))
    }

    fn remove_legacy_shortcut() -> Result<()> {
        let shortcut = legacy_startup_shortcut_path()?;
        if shortcut.exists() {
            std::fs::remove_file(&shortcut)
                .with_context(|| format!("failed to remove {}", shortcut.display()))?;
            println!(
                "Removed legacy startup shortcut at {} (superseded by the scheduled task).",
                shortcut.display()
            );
        }
        Ok(())
    }

    /// Windows has no simple native "register a scheduled task" call from
    /// Rust without COM/ITaskService FFI -- shelling out to PowerShell's
    /// ScheduledTasks module is far lighter, and consistent with already
    /// shelling out to `systemctl` on the Linux side.
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

    fn powershell_output(script: &str) -> Result<String> {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .context("failed to run powershell")?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Stop any running instance of the *installed* binary (matched by
    /// path, excluding this process). Needed before re-install: Windows
    /// won't let `fs::copy` overwrite a running executable, and without a
    /// stop the fresh launch would run alongside the old instance.
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

    /// The task definition, as Task Scheduler XML.
    ///
    /// Two triggers, both load-bearing:
    /// - `LogonTrigger` starts it promptly at logon.
    /// - `TimeTrigger`, with an indefinite one-minute repetition, is the
    ///   supervisor: it re-runs the action every minute forever, which is
    ///   what brings the service back after a crash or a kill. This is the
    ///   counterpart to the Linux unit's `Restart=on-failure`. Without it a
    ///   single death means dead until the next logon -- exactly the
    ///   failure mode the old Startup-shortcut install had.
    ///
    /// `MultipleInstancesPolicy=IgnoreNew` is what makes that repetition
    /// safe: while the process lives the task counts as running, so each
    /// minute's run is skipped. Task Scheduler supplies the single-instance
    /// guard itself -- no mutex or PID file needed.
    ///
    /// `InteractiveToken` keeps it in the logged-on user's session, which
    /// the module docs above explain is required for reliable USB
    /// notification delivery. `ExecutionTimeLimit` of `PT0S` means "no
    /// limit" -- the default would kill this long-lived process after three
    /// days.
    fn task_xml(binary: &std::path::Path) -> Result<String> {
        let user_name = std::env::var("USERNAME").context("%USERNAME% is not set")?;
        let user = match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => format!("{domain}\\{user_name}"),
            _ => user_name,
        };
        let working_dir = binary.parent().expect("binary path has a parent");
        Ok(format!(
            r#"<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>bad_kvm_switch - automatic DDC/CI monitor switching</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
    <TimeTrigger>
      <Enabled>true</Enabled>
      <StartBoundary>2020-01-01T00:00:00</StartBoundary>
      <Repetition>
        <Interval>PT1M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{}</Command>
      <WorkingDirectory>{}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>"#,
            binary.display(),
            working_dir.display(),
        ))
    }

    fn register_task(binary: &std::path::Path) -> Result<()> {
        let xml = task_xml(binary)?;
        // Handed over as a file rather than inline: the XML carries quotes
        // and newlines that would need brittle escaping through a
        // PowerShell `-Command` string.
        let xml_path = std::env::temp_dir().join("bad_kvm_switch_task.xml");
        std::fs::write(&xml_path, xml)
            .with_context(|| format!("failed to write {}", xml_path.display()))?;
        let script = format!(
            "Register-ScheduledTask -TaskName '{TASK_NAME}' \
             -Xml (Get-Content -Raw -Path '{}') -Force | Out-Null",
            xml_path.display()
        );
        let result = run_powershell(&script)
            .with_context(|| format!("failed to register scheduled task '{TASK_NAME}'"));
        let _ = std::fs::remove_file(&xml_path);
        result
    }

    fn task_exists() -> Result<bool> {
        let out = powershell_output(&format!(
            "if (Get-ScheduledTask -TaskName '{TASK_NAME}' -ErrorAction SilentlyContinue) \
             {{ 'True' }} else {{ 'False' }}"
        ))?;
        Ok(out == "True")
    }

    fn unregister_task() -> Result<bool> {
        if !task_exists()? {
            return Ok(false);
        }
        run_powershell(&format!(
            "Unregister-ScheduledTask -TaskName '{TASK_NAME}' -Confirm:$false"
        ))?;
        Ok(true)
    }

    /// Best-effort: a task that isn't registered, or isn't running, is a
    /// normal state here rather than a failure.
    fn stop_task() {
        let _ = run_powershell(&format!(
            "Stop-ScheduledTask -TaskName '{TASK_NAME}' -ErrorAction SilentlyContinue"
        ));
    }

    pub fn install() -> Result<()> {
        // Stop the task before the process: otherwise the one-minute
        // repetition could relaunch the old binary in the window between
        // killing it and copying the new one over the top.
        stop_task();
        stop_installed_instance()?;

        let binary = copy_self_to_install_dir()?;
        remove_legacy_shortcut()?;
        register_task(&binary)?;

        // Start through the task rather than spawning directly: an instance
        // Task Scheduler didn't launch isn't covered by `IgnoreNew`, so the
        // next repetition would start a second one alongside it.
        run_powershell(&format!("Start-ScheduledTask -TaskName '{TASK_NAME}'"))
            .with_context(|| format!("failed to start scheduled task '{TASK_NAME}'"))?;

        println!("Installed and started (binary copied to {}).", binary.display());
        println!(
            "Scheduled task '{TASK_NAME}' starts it at logon and re-checks every minute, so a \
             crash or a kill is recovered automatically."
        );
        println!("Inspect it any time with: Get-ScheduledTask -TaskName {TASK_NAME}");
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        stop_task();
        stop_installed_instance()?;
        let removed = unregister_task()?;
        remove_legacy_shortcut()?;
        if removed {
            println!("Removed scheduled task '{TASK_NAME}' and stopped the running instance.");
        } else {
            println!("Not installed (no scheduled task named '{TASK_NAME}').");
        }
        Ok(())
    }

    pub fn status() -> Result<()> {
        if task_exists()? {
            println!("Installed: scheduled task '{TASK_NAME}' is registered.");
        } else {
            println!("Not installed (no scheduled task named '{TASK_NAME}').");
        }

        if legacy_startup_shortcut_path()?.exists() {
            println!(
                "Note: a legacy startup shortcut is still present -- re-run `install` to remove it."
            );
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
        let is_running = powershell_output(&format!(
            "$p = Get-Process -Name bad_kvm_switch -ErrorAction SilentlyContinue | \
             Where-Object {{ $_.Id -ne {current_pid} -and $_.Path -eq '{}' }}; \
             if ($p) {{ 'True' }} else {{ 'False' }}",
            installed_path.display()
        ))? == "True";
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
