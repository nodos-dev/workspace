extern crate clap;

use std::process::ExitCode;

fn main() -> ExitCode {
    let result = nosman::nosman::cli::run_cli();
    // A command can return early from anywhere, so take down whatever progress
    // it left on screen before anything else is written.
    nosman::nosman::ui::finish_progress();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            nosman::nosman::ui::error(e);
            ExitCode::FAILURE
        }
    }
}
