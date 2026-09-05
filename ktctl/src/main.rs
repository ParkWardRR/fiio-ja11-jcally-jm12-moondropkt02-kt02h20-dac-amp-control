//! `ktctl` binary entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
    match ktctl::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
