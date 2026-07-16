mod cli;
mod usb_watch;

use clap::Parser;
use cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::List) => usb_watch::print_device_list(),
        None => {
            println!("not yet implemented — run with --help to see available subcommands");
            Ok(())
        }
    }
}
