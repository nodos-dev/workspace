use std::path::PathBuf;
use clap::{ArgMatches};
use colored::Colorize;
use native_dialog::MessageDialog;
use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

pub struct LaunchCommand {}

pub fn launch_nodos(workspace_dir: &PathBuf, hide_output: bool) {
    println!("{}", "Launching Nodos...".green());
    // Assume workspace is cwd.
    let engines_dir = nosman::path::get_default_engines_dir(workspace_dir);
    if !engines_dir.exists() {
        MessageDialog::new()
            .set_title("Nodos")
            .set_text("No installed Nodos engine found in workspace.")
            .show_alert().expect("Failed to show message dialog");
        std::process::exit(1);
    }

    let mut opt_editor_path = None;
    let mut opt_engine_path = None;
    // For each folder in engines_dir, check if it has SDK/version.json
    for entry in std::fs::read_dir(engines_dir).expect("Unable to read Engine directory") {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let binaries_dir = path.join("Binaries");
        if !binaries_dir.exists() {
            continue;
        }
        // Launch nosEditor and nosEngine
        let mut editor_path = binaries_dir.join("nosEditor");
        let mut engine_path = binaries_dir.join("nosLauncher");
        if cfg!(target_os = "windows") {
            editor_path = editor_path.with_extension("exe");
            engine_path = engine_path.with_extension("exe");
        }
        if !editor_path.exists() || !engine_path.exists() {
            continue;
        }
        opt_editor_path = Some(editor_path);
        opt_engine_path = Some(engine_path);
        break;
    }
    if opt_editor_path.is_none() || opt_engine_path.is_none() {
        MessageDialog::new()
            .set_title("Nodos")
            .set_text("No installed Nodos engine found in workspace. Check Engine folder.")
            .show_alert().unwrap();
        std::process::exit(1);
    }
    let editor_path = opt_editor_path.unwrap();
    let engine_path = opt_engine_path.unwrap();
    let mut editor_cmd = std::process::Command::new(&editor_path);
    editor_cmd.arg("--no-duplicate-instance")
        .arg("--dont-wait-engine")
        .current_dir(editor_path.parent().expect("Unable to get parent directory of nosEditor"));
    let mut engine_cmd = std::process::Command::new(&engine_path);
    engine_cmd.arg("--exit-silently-if-duplicate")
        .current_dir(engine_path.parent().expect("Unable to get parent directory of nosLauncher"));
    if hide_output {
        editor_cmd.stdout(std::process::Stdio::null());
        editor_cmd.stderr(std::process::Stdio::null());
        engine_cmd.stdout(std::process::Stdio::null());
        engine_cmd.stderr(std::process::Stdio::null());
    }
    editor_cmd.spawn().expect("Failed to launch nosEditor");
    engine_cmd.spawn().expect("Failed to launch nosLauncher");
}

impl LaunchCommand {
    fn launch_nodos(&self) -> CommandResult {
        let workspace_dir = nosman::workspace::current_root().unwrap();
        launch_nodos(workspace_dir, true);
        Ok(true)
    }
}

impl Command for LaunchCommand {
    fn matched_args<'a>(&self, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("launch")
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, _args: &ArgMatches) -> CommandResult {
        self.launch_nodos()
    }
}
