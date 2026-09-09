//! Binary entry point for `jmove`.
//!
//! All logic lives in the library crate; this main only maps the outcome of
//! [`jmove::cli::run`] onto process exit codes:
//! - `0` — success,
//! - `1` — user-facing plan/validation error (already printed),
//! - `2` — unexpected failure.

/// Parses arguments, runs the selected command and converts failures into a
/// non-zero exit code, printing a one-line message for the user.
fn main() {
    match jmove::cli::run() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("jmove: {err}");
            std::process::exit(2);
        }
    }
}
