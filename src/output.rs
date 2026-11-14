//! Everything the user reads: the `Info:` / `Success:` / `Warning:` /
//! `Error:` lines, tables and `--format json`.
//!
//! The line texts are the Python tool's, so `journalctl -u vultr-dns-updater`
//! reads the same. Colour only on a terminal (and never with `NO_COLOR`), so
//! the journal gets plain text. Warnings and errors go to stderr, keeping
//! stdout clean for `--format json`.

use std::io::IsTerminal;

/// `println!` that ignores write errors. A closed pipe (`vultr-dns usage |
/// head`) or a restarted journald must neither panic nor stop an `update`
/// halfway; the work goes on and the exit code reflects the work.
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

/// `print!` that ignores write errors (see `outln!`).
macro_rules! out {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = write!(std::io::stdout(), $($arg)*);
    }};
}

/// `eprintln!` that ignores write errors (see `outln!`).
macro_rules! errln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

pub(crate) use {out, outln};

use colored::Colorize;
use serde::Serialize;
use tabled::settings::Style;
use tabled::{Table, Tabled};

/// Decides once, at startup, whether anything gets coloured.
pub fn init_color() {
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    colored::control::set_override(std::io::stdout().is_terminal() && !no_color);
}

pub fn info(msg: &str) {
    outln!("{} {msg}", "Info:".bold().blue());
}

pub fn success(msg: &str) {
    outln!("{} {msg}", "Success:".bold().green());
}

pub fn warning(msg: &str) {
    errln!("{} {msg}", "Warning:".bold().yellow());
}

pub fn error(msg: &str) {
    errln!("{} {msg}", "Error:".bold().red());
}

/// How the read commands print their results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Format {
    /// Aligned columns (the default, even when piped).
    #[default]
    Human,
    /// One JSON document on stdout.
    Json,
}

/// Plain aligned columns, no borders.
pub fn table<T: Tabled>(rows: impl IntoIterator<Item = T>) -> String {
    Table::new(rows).with(Style::blank()).to_string()
}

pub fn print_json<T: Serialize + ?Sized>(value: &T) -> anyhow::Result<()> {
    outln!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// At most `max` bytes of `s`, cut on a character boundary, with "…" when cut.
pub fn excerpt(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// An error and its sources, joined with ": " (what anyhow's `{:#}` prints).
pub fn chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(s) = source {
        out.push_str(": ");
        out.push_str(&s.to_string());
        source = s.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_cuts_on_char_boundary() {
        assert_eq!(excerpt("  short  ", 10), "short");
        assert_eq!(excerpt("abcdef", 3), "abc…");
        assert_eq!(excerpt("ééé", 3), "é…");
    }

    #[test]
    fn table_is_plain_columns() {
        #[derive(Tabled)]
        struct Row {
            #[tabled(rename = "Name")]
            name: &'static str,
            #[tabled(rename = "TTL")]
            ttl: u32,
        }
        let t = table([
            Row {
                name: "home",
                ttl: 60,
            },
            Row {
                name: "@",
                ttl: 300,
            },
        ]);
        let lines: Vec<&str> = t.lines().map(str::trim_end).collect();
        assert_eq!(lines, vec![" Name   TTL", " home   60", " @      300"]);
    }
}
