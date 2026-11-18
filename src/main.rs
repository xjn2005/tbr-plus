#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use clap::Parser;
use tbr_plus::{LaunchOptions, run};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[cfg(feature = "gui")]
    #[clap(
        short,
        long,
        help = "Using terminal to read e-book, by default if gui exists, tbr will using gui view."
    )]
    terminal: bool,
    filename: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    run(LaunchOptions {
        #[cfg(feature = "gui")]
        terminal: cli.terminal,
        #[cfg(not(feature = "gui"))]
        terminal: true,
        filename: cli.filename,
    })
}
