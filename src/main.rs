//! Entry point for the incremental Rust replacement.

mod cli;
mod config;
mod error;
mod manifest;
mod model;
mod policy;
mod project;

use clap::Parser;

fn main() {
    let args = cli::Cli::parse();
    if let Err(error) = cli::run(args) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
