#[cfg(target_os = "windows")]
mod adl;
mod cli;
mod ddc_control;
mod usb_watch;

use clap::Parser;
use cli::{Cli, Command};

// Hardcoded until M5 adds config-file support.
// The NK65 keyboard, which goes through the KVM switch (confirmed against
// `--list` while toggling the switch).
const TARGET_VID: u16 = 0x8968;
const TARGET_PID: u16 = 0x4e4b;
// This PC's own monitor input, in LG's DDC alt-mode encoding (feature
// 0xF4), on the ADL adapter/display confirmed working in M2.
#[cfg(target_os = "windows")]
const MY_INPUT_ADAPTER: i32 = 5;
#[cfg(target_os = "windows")]
const MY_INPUT_DISPLAY: i32 = 0;
#[cfg(target_os = "windows")]
const MY_INPUT_VALUE: u8 = 0xd0; // DisplayPort

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::List) => usb_watch::print_device_list(),
        Some(Command::Watch) => usb_watch::watch(TARGET_VID, TARGET_PID, || {}),
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
        #[cfg(target_os = "windows")]
        None => usb_watch::watch(TARGET_VID, TARGET_PID, || {
            if let Err(e) = adl::set_vcp_alt_mode(0xf4, MY_INPUT_VALUE, MY_INPUT_ADAPTER, MY_INPUT_DISPLAY) {
                eprintln!("failed to switch monitor input: {e:?}");
            }
        }),
        #[cfg(not(target_os = "windows"))]
        None => {
            anyhow::bail!(
                "no default run mode yet for this OS -- the LG alt-mode switch is Windows/AMD-only so far (Linux support is a later milestone). Run with --help to see available debug subcommands."
            )
        }
    }
}
