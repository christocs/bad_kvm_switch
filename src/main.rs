#[cfg(target_os = "windows")]
mod adl;
mod cli;
mod config;
mod ddc_control;
#[cfg(target_os = "linux")]
mod linux_i2c;
#[cfg(target_os = "windows")]
mod nvapi;
mod service;
mod usb_watch;

use clap::Parser;
use cli::{Cli, Command};
use config::{Config, SwitchMethod};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::List) => usb_watch::print_device_list(),
        Some(Command::Watch) => {
            let config = load_config(cli.config.as_deref())?;
            // Console-only logging: without a subscriber, the watch loop's
            // tracing events would be silently dropped and this debug
            // command would print nothing at all.
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_level))
                .with_ansi(stdout_supports_ansi())
                .init();
            usb_watch::watch(config.usb_vendor_id, config.usb_product_id, || {})
        }
        Some(Command::DdcGet { feature }) => {
            let value = ddc_control::get_vcp(feature)?;
            println!("0x{feature:02x} = 0x{value:04x} ({value})");
            Ok(())
        }
        Some(Command::DdcSet { feature, value }) => {
            ddc_control::set_vcp(feature, value)?;
            println!("0x{feature:02x} set to 0x{value:04x} ({value})");
            Ok(())
        }
        #[cfg(target_os = "windows")]
        Some(Command::DdcAdlProbe) => adl::probe_displays(),
        #[cfg(not(target_os = "windows"))]
        Some(Command::DdcAdlProbe) => {
            anyhow::bail!("ddc-adl-probe is Windows/AMD-only (uses ADL)")
        }
        #[cfg(target_os = "windows")]
        Some(Command::DdcAdlSet {
            feature,
            value,
            adapter,
            display,
        }) => {
            adl::set_vcp_alt_mode(feature, value, adapter, display)?;
            println!(
                "0x{feature:02x} alt-mode set to 0x{value:02x} via ADL adapter={adapter} display={display}"
            );
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        Some(Command::DdcAdlSet { .. }) => {
            anyhow::bail!("ddc-adl-set is Windows/AMD-only (uses ADL)")
        }
        #[cfg(target_os = "linux")]
        Some(Command::DdcLinuxAltSet { feature, value }) => {
            linux_i2c::set_vcp_alt_mode(feature, value)?;
            println!("0x{feature:02x} alt-mode set to 0x{value:02x} via raw I2C");
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        Some(Command::DdcLinuxAltSet { .. }) => {
            anyhow::bail!("ddc-linux-alt-set is Linux-only (uses raw i2c-dev)")
        }
        #[cfg(target_os = "windows")]
        Some(Command::DdcNvapiProbe) => nvapi::probe(),
        #[cfg(not(target_os = "windows"))]
        Some(Command::DdcNvapiProbe) => {
            anyhow::bail!("ddc-nvapi-probe is Windows/NVIDIA-only (uses NvAPI)")
        }
        #[cfg(target_os = "windows")]
        Some(Command::DdcNvapiSet { feature, value }) => {
            nvapi::set_vcp_alt_mode(feature, value)?;
            println!("0x{feature:02x} alt-mode set to 0x{value:02x} via NvAPI");
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        Some(Command::DdcNvapiSet { .. }) => {
            anyhow::bail!("ddc-nvapi-set is Windows/NVIDIA-only (uses NvAPI)")
        }
        Some(Command::Install) => service::install(),
        Some(Command::Uninstall) => service::uninstall(),
        Some(Command::Status) => service::status(),
        None => run_service(cli.config.as_deref()),
    }
}

/// The real run mode: watch for the configured peripheral and switch the
/// monitor input on every connect event.
fn run_service(config_path: Option<&std::path::Path>) -> anyhow::Result<()> {
    let config = load_config(config_path)?;

    // Keep the file-writer guard alive for the process's lifetime --
    // dropping it stops the background thread that flushes file logs.
    let _log_guard = init_logging(&config.log_level)?;

    // Exit cleanly (code 0, with a log line) on Ctrl+C -- and, via the
    // `termination` feature, on SIGTERM too, which is what
    // `systemctl --user stop` sends. Without this, an interactive Ctrl+C
    // reports an error-looking exit status on Windows
    // (STATUS_CONTROL_C_EXIT). The handler runs on its own thread, so a
    // plain exit() is the simplest correct way out of the blocking watch
    // loop; there's no state needing cleanup mid-loop.
    ctrlc::set_handler(|| {
        tracing::info!("shutting down");
        std::process::exit(0);
    })
    .expect("failed to set Ctrl+C handler");

    tracing::info!(
        "starting: watching {:04x}:{:04x}, switch_method {:?}",
        config.usb_vendor_id,
        config.usb_product_id,
        config.switch_method
    );

    usb_watch::watch(config.usb_vendor_id, config.usb_product_id, move || {
        switch_with_retry(&config);
    })
}

fn load_config(path: Option<&std::path::Path>) -> anyhow::Result<Config> {
    Config::load(path).map_err(anyhow::Error::from)
}

/// Console + rotating file logging (daily, capped file count, in the
/// platform data dir alongside the installed binary). The file log is
/// what makes the installed service debuggable at all: it runs with a
/// hidden/no console on Windows and detached under systemd on Linux, so
/// without a file there'd be no trace of why it misbehaved or died.
fn init_logging(log_level: &str) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let log_dir = directories::ProjectDirs::from("", "", "bad_kvm_switch")
        .map(|dirs| dirs.data_local_dir().join("logs"))
        .ok_or_else(|| anyhow::anyhow!("could not determine a data directory for this platform"))?;

    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("bad_kvm_switch")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&log_dir)
        .map_err(|e| {
            anyhow::anyhow!("failed to set up log directory {}: {e}", log_dir.display())
        })?;
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(log_level))
        .with(tracing_subscriber::fmt::layer().with_ansi(stdout_supports_ansi()))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false),
        )
        .init();

    Ok(guard)
}

/// True only if stdout is a real terminal that can render ANSI. On Windows
/// this also enables virtual-terminal processing so colors work in legacy
/// cmd/conhost sessions, not just Windows Terminal; if that can't be enabled
/// we return false so we emit plain text instead of raw escape codes.
fn stdout_supports_ansi() -> bool {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        enable_ansi_support::enable_ansi_support().is_ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

/// Retry wrapper around the actual switch: DDC/CI over I2C (or the OS's
/// DDC layer) can transiently NACK, and right after a KVM press the
/// monitor may be busy re-syncing. A couple of spaced retries cover that
/// without any risk -- re-sending "switch to me" is idempotent.
fn switch_with_retry(config: &Config) {
    const ATTEMPTS: u32 = 3;
    for attempt in 1..=ATTEMPTS {
        match switch_to_my_input(config) {
            Ok(()) => {
                tracing::info!("switch succeeded on attempt {attempt}/{ATTEMPTS}");
                return;
            }
            Err(e) if attempt < ATTEMPTS => {
                tracing::warn!("switch attempt {attempt}/{ATTEMPTS} failed: {e:#}; retrying");
                std::thread::sleep(Duration::from_millis(500 * u64::from(attempt)));
            }
            Err(e) => {
                tracing::error!("failed to switch monitor input after {ATTEMPTS} attempts: {e:#}");
            }
        }
    }
}

fn switch_to_my_input(config: &Config) -> anyhow::Result<()> {
    match config.switch_method {
        SwitchMethod::Standard => {
            ddc_control::set_vcp_if_needed(config.vcp_feature, config.input_value)
        }
        SwitchMethod::LgAltMode => switch_lg_alt_mode(config),
    }
}

#[cfg(target_os = "windows")]
fn switch_lg_alt_mode(config: &Config) -> anyhow::Result<()> {
    match config.alt_mode_backend {
        config::AltModeBackend::Amd => adl::set_vcp_alt_mode(
            config.vcp_feature,
            config.alt_mode_input_value(),
            config.adl_adapter,
            config.adl_display,
        ),
        config::AltModeBackend::Nvidia => {
            nvapi::set_vcp_alt_mode(config.vcp_feature, config.alt_mode_input_value())
        }
    }
}

#[cfg(target_os = "linux")]
fn switch_lg_alt_mode(config: &Config) -> anyhow::Result<()> {
    linux_i2c::set_vcp_alt_mode(config.vcp_feature, config.alt_mode_input_value())
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn switch_lg_alt_mode(_config: &Config) -> anyhow::Result<()> {
    anyhow::bail!(
        "switch_method = \"lg_alt_mode\" is only implemented for Windows and Linux so far"
    )
}
