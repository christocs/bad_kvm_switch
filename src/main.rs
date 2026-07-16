#[cfg(target_os = "windows")]
mod adl;
mod cli;
mod config;
mod ddc_control;
mod usb_watch;

use clap::Parser;
use cli::{Cli, Command};
use config::{Config, SwitchMethod};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::List) => usb_watch::print_device_list(),
        Some(Command::Watch) => {
            let config = load_config(cli.config.as_deref())?;
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
        Some(Command::DdcAdlSet { feature, value, adapter, display }) => {
            adl::set_vcp_alt_mode(feature, value, adapter, display)?;
            println!("0x{feature:02x} alt-mode set to 0x{value:02x} via ADL adapter={adapter} display={display}");
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        Some(Command::DdcAdlSet { .. }) => {
            anyhow::bail!("ddc-adl-set is Windows/AMD-only (uses ADL)")
        }
        None => {
            let config = load_config(cli.config.as_deref())?;
            init_logging(&config.log_level);
            usb_watch::watch(config.usb_vendor_id, config.usb_product_id, move || {
                if let Err(e) = switch_to_my_input(&config) {
                    tracing::error!("failed to switch monitor input: {e:?}");
                }
            })
        }
    }
}

fn load_config(path: Option<&std::path::Path>) -> anyhow::Result<Config> {
    Config::load(path).map_err(anyhow::Error::from)
}

fn init_logging(log_level: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(log_level))
        .init();
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
    adl::set_vcp_alt_mode(
        config.vcp_feature,
        config.input_value as u8,
        config.adl_adapter,
        config.adl_display,
    )
}

#[cfg(not(target_os = "windows"))]
fn switch_lg_alt_mode(_config: &Config) -> anyhow::Result<()> {
    anyhow::bail!("switch_method = \"lg_alt_mode\" is Windows/AMD-only so far (uses ADL)")
}
