use clap::Parser;

/// Automatically switch a monitor's DDC/CI input based on which PC a shared
/// USB peripheral is currently plugged into.
#[derive(Parser, Debug)]
#[command(name = "bad_kvm_switch", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// List currently-attached USB devices (vendor:product, manufacturer, product, serial).
    ///
    /// Use this to find the VID:PID of the keyboard/mouse you want to watch for.
    List,

    /// Read a VCP feature's current value from the first detected monitor (debug tool).
    DdcGet {
        /// VCP feature code, hex (e.g. 0x60, 60, F4).
        #[arg(value_parser = parse_hex_u8)]
        feature: u8,
    },

    /// Write a VCP feature's value on the first detected monitor (debug tool).
    DdcSet {
        /// VCP feature code, hex (e.g. 0x60, 60, F4).
        #[arg(value_parser = parse_hex_u8)]
        feature: u8,
        /// Value to write, hex (e.g. 0x0f, D0).
        #[arg(value_parser = parse_hex_u16)]
        value: u16,
    },

    /// List connected displays and their adapter/display indices via the
    /// legacy AMD ADL SDK (Windows/AMD only; debug tool).
    DdcAdlProbe,

    /// Write a VCP feature via LG's DDC alt-mode side channel, using the
    /// legacy AMD ADL SDK's DDC block access (Windows/AMD only; debug tool).
    DdcAdlSet {
        /// VCP feature code, hex (e.g. 0xF4).
        #[arg(value_parser = parse_hex_u8)]
        feature: u8,
        /// Value to write, hex (e.g. 0x90).
        #[arg(value_parser = parse_hex_u8)]
        value: u8,
        /// ADL adapter index (see ddc-adl-probe).
        #[arg(long, default_value_t = 0)]
        adapter: i32,
        /// ADL logical display index (see ddc-adl-probe).
        #[arg(long, default_value_t = 0)]
        display: i32,
    },
}

fn parse_hex_u8(s: &str) -> Result<u8, String> {
    let trimmed = s.trim_start_matches("0x").trim_start_matches("0X");
    u8::from_str_radix(trimmed, 16).map_err(|e| format!("invalid hex byte '{s}': {e}"))
}

fn parse_hex_u16(s: &str) -> Result<u16, String> {
    let trimmed = s.trim_start_matches("0x").trim_start_matches("0X");
    u16::from_str_radix(trimmed, 16).map_err(|e| format!("invalid hex value '{s}': {e}"))
}
