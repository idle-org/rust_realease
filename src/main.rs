//! Entry point for the incremental Rust replacement.

mod build;
mod cli;
mod config;
mod error;
mod gitea;
mod manifest;
mod model;
mod package;
mod policy;
mod prepare;
mod project;
mod runner;
mod ui;
mod verify;

use clap::Parser;

fn main() {
    let args = cli::Cli::parse();
    if let Err(error) = cli::run(args) {
        ui::error(error);
        std::process::exit(1);
    }
}
