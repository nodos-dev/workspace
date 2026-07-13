use std::path::{Path, PathBuf};
use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use sysinfo::{Pid, System};
use crate::nosman;
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::launch::{
    launch_engine, launch_nodos, list_engines, select_engine,
};
use crate::nosman::workspace::Workspace;

/// Base names (without platform extension) of the processes that make up a
/// running Nodos instance (editor, launcher, engine). Discovery is stateless: we
/// enumerate live processes and match these names, then scope to the current
/// workspace by exe path.
const NODOS_PROCESS_NAMES: [&str; 3] = ["nosEditor", "nosLauncher", "nosEngine"];

/// A live Nodos process discovered under the current workspace.
struct RunningProcess {
    pid: Pid,
    /// Matched base name (e.g. "nosEditor").
    name: String,
    /// Seconds the process has been running.
    run_time: u64,
}

fn refresh_system() -> System {
    let mut sys = System::new_all();
    sys.refresh_all();
    sys
}

/// Returns true if `name` (a live process name, possibly with a `.exe` suffix on
/// Windows) refers to one of the known Nodos process base names.
fn match_nodos_name(name: &str) -> Option<&'static str> {
    NODOS_PROCESS_NAMES.iter().find_map(|base| {
        if name.eq_ignore_ascii_case(base) || name.eq_ignore_ascii_case(&format!("{}.exe", base)) {
            Some(*base)
        } else {
            None
        }
    })
}

/// Enumerate the Nodos processes whose executable lives under this workspace's
/// engines directory. Scoping by path ensures we never touch a Nodos instance
/// belonging to another workspace.
fn find_nodos_processes(sys: &System, engines_dir: &Path) -> Vec<RunningProcess> {
    let canon_engines = dunce::canonicalize(engines_dir).unwrap_or_else(|_| engines_dir.to_path_buf());
    let mut found = Vec::new();
    for (pid, process) in sys.processes() {
        let pname = process.name().to_string_lossy();
        let base = match match_nodos_name(&pname) {
            Some(b) => b,
            None => continue,
        };
        // Scope to this workspace via the executable path.
        let exe = match process.exe() {
            Some(e) => e,
            None => continue,
        };
        let under_workspace = dunce::canonicalize(exe)
            .map(|p| p.starts_with(&canon_engines))
            .unwrap_or_else(|_| exe.starts_with(engines_dir));
        if !under_workspace {
            continue;
        }
        found.push(RunningProcess {
            pid: *pid,
            name: base.to_string(),
            run_time: process.run_time(),
        });
    }
    // Stable order: editor, launcher, engine.
    found.sort_by_key(|p| {
        NODOS_PROCESS_NAMES.iter().position(|n| *n == p.name).unwrap_or(usize::MAX)
    });
    found
}

fn format_uptime(seconds: u64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}

/// Terminate a process, gracefully by default. On Unix this sends SIGTERM
/// (SIGKILL when forced); Windows has no graceful kernel-level signal, so both
/// paths call the platform terminate.
#[cfg(unix)]
fn terminate(process: &sysinfo::Process, force: bool) -> bool {
    if force {
        process.kill()
    } else {
        process.kill_with(sysinfo::Signal::Term).unwrap_or_else(|| process.kill())
    }
}

#[cfg(not(unix))]
fn terminate(process: &sysinfo::Process, _force: bool) -> bool {
    process.kill()
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("engine")
        .about("Manage the local Nodos engine (launch/stop/status/restart)")
        .subcommand(clap::Command::new("launch")
            .about("Launch Nodos")
            .arg(crate::nosman::command::launch::get_engine_arg()))
        .subcommand(clap::Command::new("list")
            .about("List the engines installed in this workspace")
            .arg(Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Print the engine list as JSON")))
        .subcommand(clap::Command::new("stop")
            .about("Stop the running Nodos engine and editor for this workspace")
            .arg(Arg::new("force")
                .long("force")
                .short('f')
                .action(ArgAction::SetTrue)
                .help("Forcefully kill the processes instead of requesting a graceful shutdown")))
        .subcommand(clap::Command::new("status")
            .about("Show whether the Nodos engine and editor are running for this workspace")
            .arg(Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Print the status as JSON")))
        .subcommand(clap::Command::new("restart")
            .about("Stop the running Nodos instance (if any) and launch it again")
            .arg(crate::nosman::command::launch::get_engine_arg())
            .arg(Arg::new("force")
                .long("force")
                .short('f')
                .action(ArgAction::SetTrue)
                .help("Forcefully kill the processes during the stop phase")))
}

/// Stop all discovered Nodos processes for the workspace. Returns how many were
/// asked to terminate.
fn stop_processes(workspace_dir: &PathBuf, force: bool) -> usize {
    let engines_dir = nosman::path::get_default_engines_dir(workspace_dir);
    let sys = refresh_system();
    let procs = find_nodos_processes(&sys, &engines_dir);
    let mut stopped = 0;
    for p in &procs {
        if let Some(process) = sys.process(p.pid) {
            if terminate(process, force) {
                println!("{} {} (pid {})", "Stopped".green(), p.name.cyan(), p.pid);
                stopped += 1;
            } else {
                println!("{} {} (pid {})", "Failed to stop".red(), p.name.cyan(), p.pid);
            }
        }
    }
    stopped
}

pub struct EngineLaunchCommand {}

impl Command for EngineLaunchCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("engine")?.subcommand_matches("launch")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        launch_nodos(&workspace.root, true, args.get_one::<String>("engine").map(|s| s.as_str()), false)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}

pub struct EngineListCommand {}

impl Command for EngineListCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("engine")?.subcommand_matches("list")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let engines = list_engines(&workspace.root);
        if args.get_flag("json") {
            let entries: Vec<serde_json::Value> = engines.iter().map(|e| serde_json::json!({
                "name": e.name,
                "version": e.version,
                "path": e.path,
            })).collect();
            println!("{}", serde_json::to_string_pretty(&entries).unwrap());
            return Ok(());
        }
        if engines.is_empty() {
            println!("{}", "No installed Nodos engine found in workspace.".yellow());
            return Ok(());
        }
        println!("{}", "Installed engines:".green());
        for e in &engines {
            println!("  {} ({})", e.to_string().cyan(), e.path.display());
        }
        Ok(())
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}

pub struct EngineStopCommand {}

impl Command for EngineStopCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("engine")?.subcommand_matches("stop")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let force = args.get_flag("force");
        let stopped = stop_processes(&workspace.root, force);
        if stopped == 0 {
            println!("{}", "No running Nodos engine found for this workspace.".yellow());
        }
        Ok(())
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}

pub struct EngineStatusCommand {}

impl EngineStatusCommand {
    fn run_status(&self, workspace_dir: &PathBuf, json: bool) -> CommandResult {
        let engines_dir = nosman::path::get_default_engines_dir(workspace_dir);
        let sys = refresh_system();
        let procs = find_nodos_processes(&sys, &engines_dir);

        if json {
            let entries: Vec<serde_json::Value> = procs.iter().map(|p| serde_json::json!({
                "name": p.name,
                "pid": p.pid.as_u32(),
                "uptime_seconds": p.run_time,
            })).collect();
            let out = serde_json::json!({
                "running": !procs.is_empty(),
                "processes": entries,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
            return Ok(());
        }

        if procs.is_empty() {
            println!("{}", "Nodos is not running for this workspace.".yellow());
            return Ok(());
        }
        println!("{}", "Nodos is running:".green());
        for p in &procs {
            println!(
                "  {} (pid {}, up {})",
                p.name.cyan(),
                p.pid,
                format_uptime(p.run_time),
            );
        }
        Ok(())
    }
}

impl Command for EngineStatusCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("engine")?.subcommand_matches("status")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let json = args.get_flag("json");
        self.run_status(&workspace.root, json)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}

pub struct EngineRestartCommand {}

impl Command for EngineRestartCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("engine")?.subcommand_matches("restart")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        // Resolve interactive selection before stopping the current instance so
        // cancellation cannot turn a restart into a stop.
        let engine = select_engine(
            &workspace.root,
            args.get_one::<String>("engine").map(String::as_str),
            false,
        )?;
        let force = args.get_flag("force");
        let stopped = stop_processes(&workspace.root, force);

        // Wait for the processes to actually exit before relaunching so the new
        // instance doesn't trip over the old one (duplicate-instance guards, held
        // ports, etc.).
        if stopped > 0 {
            let engines_dir = nosman::path::get_default_engines_dir(&workspace.root);
            for _ in 0..50 {
                let sys = refresh_system();
                if find_nodos_processes(&sys, &engines_dir).is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        launch_engine(engine, true)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
