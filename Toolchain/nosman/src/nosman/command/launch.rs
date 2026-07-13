use std::fmt;
use std::path::PathBuf;
use clap::{Arg, ArgMatches};
use colored::Colorize;
use inquire::Select;
use native_dialog::DialogBuilder;
use crate::nosman;
use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::index::SemVer;
use crate::nosman::workspace::Workspace;

pub struct LaunchCommand {}

/// An installed engine under the workspace's Engine directory.
pub struct EngineInfo {
    /// Folder name under the Engine directory.
    pub name: String,
    /// Version read from SDK/info.json, if present.
    pub version: Option<String>,
    pub path: PathBuf,
    pub editor_path: PathBuf,
    pub launcher_path: PathBuf,
}

impl fmt::Display for EngineInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.version {
            Some(v) if *v != self.name => write!(f, "{} ({})", self.name, v),
            _ => write!(f, "{}", self.name),
        }
    }
}

/// Enumerate launchable engines (folders with Binaries/nosEditor + nosLauncher),
/// sorted latest version first.
pub fn list_engines(workspace_dir: &PathBuf) -> Vec<EngineInfo> {
    let engines_dir = nosman::path::get_default_engines_dir(workspace_dir);
    let mut engines = Vec::new();
    let entries = match std::fs::read_dir(&engines_dir) {
        Ok(e) => e,
        Err(_) => return engines,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let binaries_dir = path.join("Binaries");
        let mut editor_path = binaries_dir.join("nosEditor");
        let mut launcher_path = binaries_dir.join("nosLauncher");
        if cfg!(target_os = "windows") {
            editor_path = editor_path.with_extension("exe");
            launcher_path = launcher_path.with_extension("exe");
        }
        if !editor_path.exists() || !launcher_path.exists() {
            continue;
        }
        let version = std::fs::read_to_string(path.join("SDK").join("info.json")).ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|j| j.get("version").and_then(|v| v.as_str()).map(String::from));
        engines.push(EngineInfo {
            name: entry.file_name().to_string_lossy().to_string(),
            version,
            path,
            editor_path,
            launcher_path,
        });
    }
    engines.sort_by(|a, b| {
        let key = |e: &EngineInfo| e.version.as_deref().and_then(SemVer::parse_from_str);
        key(b).cmp(&key(a)).then_with(|| a.name.cmp(&b.name))
    });
    engines
}

fn matches_query(engine: &EngineInfo, query: &str) -> bool {
    if engine.name.eq_ignore_ascii_case(query) {
        return true;
    }
    match &engine.version {
        Some(v) => v == query || v.starts_with(&format!("{}.", query)),
        None => false,
    }
}

fn pick_engine_tui(engines: Vec<EngineInfo>) -> Result<EngineInfo, CommandError> {
    Select::new("Select an engine to launch:", engines)
        .prompt()
        .map_err(|e| CommandError::Runtime { message: format!("No engine selected: {}", e) })
}

/// Pick an engine via a native list dialog whose platform GUI facilities are
/// loaded at runtime — on headless machines the dialog is unavailable and we
/// fall through gracefully.
fn pick_engine_dialog(mut engines: Vec<EngineInfo>) -> Result<EngineInfo, CommandError> {
    let labels: Vec<String> = engines.iter().map(|e| e.to_string()).collect();
    match nosman::dialog::select_from_list("Nodos", "Select an engine to launch:", &labels) {
        Some(index) => Ok(engines.remove(index)),
        None => Err(CommandError::Runtime { message: "No engine selected.".to_string() }),
    }
}

fn show_error(message: &str, use_dialog: bool) {
    if use_dialog {
        let _ = DialogBuilder::message()
            .set_title("Nodos")
            .set_text(message)
            .alert()
            .show();
    }
}

/// Launch a Nodos engine. `engine_query` selects by folder name or version
/// (prefix); when absent and multiple engines are installed, the user picks one
/// via a native dialog (`use_dialog`, for double-click launches) or a TUI list.
pub fn launch_nodos(workspace_dir: &PathBuf, hide_output: bool, engine_query: Option<&str>, use_dialog: bool) -> CommandResult {
    let mut engines = list_engines(workspace_dir);
    if engines.is_empty() {
        let message = "No installed Nodos engine found in workspace. Check Engine folder.";
        show_error(message, use_dialog);
        return Err(CommandError::Runtime { message: message.to_string() });
    }
    let engine = match engine_query {
        Some(query) => {
            match engines.into_iter().find(|e| matches_query(e, query)) {
                Some(e) => e,
                None => {
                    let message = format!("No engine matching '{}' found in workspace.", query);
                    show_error(&message, use_dialog);
                    return Err(CommandError::InvalidArgument { message });
                }
            }
        }
        None if engines.len() == 1 => engines.remove(0),
        None => {
            if use_dialog {
                pick_engine_dialog(engines)?
            } else {
                pick_engine_tui(engines)?
            }
        }
    };
    println!("{} {}", "Launching Nodos".green(), engine.to_string().cyan());
    let mut editor_cmd = std::process::Command::new(&engine.editor_path);
    editor_cmd.arg("--no-duplicate-instance")
        .arg("--dont-wait-engine")
        .current_dir(engine.editor_path.parent().expect("Unable to get parent directory of nosEditor"));
    let mut engine_cmd = std::process::Command::new(&engine.launcher_path);
    engine_cmd.arg("--exit-silently-if-duplicate")
        .current_dir(engine.launcher_path.parent().expect("Unable to get parent directory of nosLauncher"));
    if hide_output {
        editor_cmd.stdout(std::process::Stdio::null());
        editor_cmd.stderr(std::process::Stdio::null());
        engine_cmd.stdout(std::process::Stdio::null());
        engine_cmd.stderr(std::process::Stdio::null());
    }
    editor_cmd.spawn().unwrap_or_else(|e| panic!("Failed to launch nosEditor: {}", e));
    engine_cmd.spawn().unwrap_or_else(|e| panic!("Failed to launch nosLauncher: {}", e));
    Ok(())
}

pub fn get_engine_arg() -> Arg {
    Arg::new("engine")
        .help("Name or version of the engine to launch. If omitted and multiple engines are installed, you will be asked to pick one.")
        .required(false)
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("launch")
        .about("Launch Nodos (alias of 'engine launch')")
        .arg(get_engine_arg())
}

impl Command for LaunchCommand {
    fn matched_args<'b>(&self, _workspace: &Workspace, args : &'b ArgMatches) -> Option<&'b ArgMatches> {
        args.subcommand_matches("launch")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        launch_nodos(&workspace.root, true, args.get_one::<String>("engine").map(|s| s.as_str()), false)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
