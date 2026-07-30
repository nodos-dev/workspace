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
        let key = |e: &EngineInfo| {
            e.version
                .as_deref()
                .and_then(SemVer::parse_from_str)
                .map(|v| (
                    v.major,
                    v.minor.unwrap_or(0),
                    v.patch.unwrap_or(0),
                    v.build_number.unwrap_or(0),
                ))
        };
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

/// Pick an engine via the platform's native GUI facilities. On headless
/// machines the dialog reports no selection.
fn pick_engine_dialog(mut engines: Vec<EngineInfo>) -> Result<EngineInfo, CommandError> {
    let labels: Vec<String> = engines.iter().map(|e| e.to_string()).collect();
    match nosman::dialog::select_from_list("Nodos", "Select an engine to launch:", &labels) {
        Some(index) if index < engines.len() => Ok(engines.remove(index)),
        Some(_) => Err(CommandError::Runtime {
            message: "Engine selector returned an invalid selection.".to_string(),
        }),
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

/// Resolve an engine by folder name or version prefix. When no query is given
/// and multiple engines are installed, ask via a native dialog (`use_dialog`,
/// for double-click launches) or a TUI list.
pub fn select_engine(
    workspace_dir: &PathBuf,
    engine_query: Option<&str>,
    use_dialog: bool,
) -> Result<EngineInfo, CommandError> {
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
    Ok(engine)
}

/// `engine_args` are forwarded verbatim to nosLauncher.
pub fn launch_engine(engine: EngineInfo, hide_output: bool, engine_args: &[String]) -> CommandResult {
    println!("{} {}", "Launching Nodos".green(), engine.to_string().cyan());
    let mut editor_cmd = std::process::Command::new(&engine.editor_path);
    editor_cmd.arg("--no-duplicate-instance")
        .arg("--dont-wait-engine")
        .current_dir(engine.editor_path.parent().expect("Unable to get parent directory of nosEditor"));
    let mut engine_cmd = std::process::Command::new(&engine.launcher_path);
    engine_cmd.arg("--exit-silently-if-duplicate")
        .args(engine_args)
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

pub fn launch_nodos(
    workspace_dir: &PathBuf,
    hide_output: bool,
    engine_query: Option<&str>,
    use_dialog: bool,
    engine_args: &[String],
) -> CommandResult {
    let engine = select_engine(workspace_dir, engine_query, use_dialog)?;
    launch_engine(engine, hide_output, engine_args)
}

pub fn get_engine_arg() -> Arg {
    Arg::new("engine")
        .help("Name or version of the engine to launch. If omitted and multiple engines are installed, you will be asked to pick one.")
        .required(false)
}

/// Everything after `--`, passed on to nosLauncher.
pub fn get_engine_args_arg() -> Arg {
    Arg::new("engine_args")
        .help("Arguments after '--' are forwarded to the engine launcher")
        .last(true)
        .num_args(0..)
        .allow_hyphen_values(true)
}

/// Trailing arguments collected by [`get_engine_args_arg`].
pub fn get_engine_args(args: &ArgMatches) -> Vec<String> {
    args.get_many::<String>("engine_args")
        .map(|values| values.cloned().collect())
        .unwrap_or_default()
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("launch")
        .about("Launch Nodos (alias of 'engine launch')")
        .arg(get_engine_arg())
        .arg(get_engine_args_arg())
}

impl Command for LaunchCommand {
    fn matched_args<'b>(&self, _workspace: &Workspace, args : &'b ArgMatches) -> Option<&'b ArgMatches> {
        args.subcommand_matches("launch")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        launch_nodos(&workspace.root, true, args.get_one::<String>("engine").map(|s| s.as_str()), false, &get_engine_args(args))
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn add_engine(root: &std::path::Path, name: &str, version: &str) {
        let engine = root.join("Engine").join(name);
        fs::create_dir_all(engine.join("Binaries")).unwrap();
        fs::create_dir_all(engine.join("SDK")).unwrap();
        let extension = if cfg!(windows) { ".exe" } else { "" };
        fs::write(
            engine.join("Binaries").join(format!("nosEditor{}", extension)),
            [],
        ).unwrap();
        fs::write(
            engine.join("Binaries").join(format!("nosLauncher{}", extension)),
            [],
        ).unwrap();
        fs::write(
            engine.join("SDK").join("info.json"),
            serde_json::json!({ "version": version }).to_string(),
        ).unwrap();
    }

    #[test]
    fn list_engines_sorts_build_versions_latest_first() {
        let workspace = tempfile::tempdir().unwrap();
        add_engine(workspace.path(), "z-base", "1.4.0");
        add_engine(workspace.path(), "a-build", "1.4.0.b5");

        let engines = list_engines(&workspace.path().to_path_buf());
        let versions: Vec<_> = engines.iter().map(|e| e.version.as_deref().unwrap()).collect();

        assert_eq!(versions, ["1.4.0.b5", "1.4.0"]);
    }

    #[test]
    fn select_engine_uses_latest_matching_version_prefix() {
        let workspace = tempfile::tempdir().unwrap();
        add_engine(workspace.path(), "z-base", "1.4.0");
        add_engine(workspace.path(), "a-build", "1.4.0.b5");

        let engine =
            select_engine(&workspace.path().to_path_buf(), Some("1.4"), false).unwrap();

        assert_eq!(engine.version.as_deref(), Some("1.4.0.b5"));
    }
}
