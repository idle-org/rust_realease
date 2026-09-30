use console::{style, Style};
use std::fmt::Display;

pub fn detail(label: &str, value: impl Display) {
    println!("{}: {value}", style(label).cyan().bold());
}

pub fn success(message: impl Display) {
    println!("{} {message}", style("✓").green().bold());
}

pub fn warning(message: impl Display) {
    println!("{} {message}", style("!").yellow().bold());
}

pub fn error(message: impl Display) {
    let marker = Style::new().red().bold().for_stderr().apply_to("error:");
    eprintln!("{marker} {message}");
}
