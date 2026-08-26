extern crate clap;

use std::process::ExitCode;

fn main() -> ExitCode {
    match nosman::nosman::cli::run_cli() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            nosman::nosman::ui::error(e);
            ExitCode::FAILURE
        }
    }
}
