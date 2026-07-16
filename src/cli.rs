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
}
