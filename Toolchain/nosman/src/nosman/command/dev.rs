use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use include_dir::{include_dir, Dir};
use indicatif::ProgressBar;
use rayon::prelude::*;
use CommandError::InvalidArgument;
use crate::nosman::common::copy_include_dir_recursive;
use crate::nosman::command::{get_lang_tool_arg, Command, CommandError, CommandResult};
use crate::nosman::lang_tool::LangTool;
use crate::nosman::workspace::Workspace;

pub fn get_cli() -> clap::Command {
    let git_dir_arg = Arg::new("dir")
        .long("directory")
        .short('m')
        .help("Path to the directory to scan for git repositories")
        .action(ArgAction::Append)
        .num_args(1)
        .default_values(&[".", "Engine", "Module"]);
    clap::Command::new("dev")
        .about("Helper commands for Nodos module development")
        .subcommand(clap::Command::new("pull")
            .about("Scans for git repositories and pulls their current branches")
            .arg(git_dir_arg.clone())
        )
        .subcommand(clap::Command::new("gen")
            .about("Generates project files for Nodos plugin development")
            .arg(get_lang_tool_arg())
            .arg(Arg::new("project_folder")
                .long("project-folder")
                .short('p')
                .help("Path to the project folder to generate files in")
                .default_value("Project"))
            .arg(Arg::new("plugin_dirs")
                .long("plugin-dirs")
                .alias("module_dirs")
                .help("Plugin directories to generate if only one of them is wanted")
                .num_args(0..=1) // 0 or 1 argument allowed
                .allow_hyphen_values(true))
            .arg(Arg::new("extra_args")
                .trailing_var_arg(true)
                .num_args(1..)
                .allow_hyphen_values(true)
                .help("Arguments to pass to the underlying tool when generating project files")
            )
        )
        .subcommand(clap::Command::new("build")
            .about("Builds project files for Nodos module development")
            .arg(get_lang_tool_arg())
            .arg(Arg::new("project_folder")
                .long("project-folder")
                .short('p')
                .help("Path to the project folder to build")
                .default_value("Project"))
            .arg(Arg::new("config")
                .long("config")
                .help("CMake-only build configuration (e.g. Debug/Release)"))
            .arg(Arg::new("target")
                .long("target")
                .help("CMake-only build target"))
            .arg(Arg::new("clean_first")
                .long("clean-first")
                .action(ArgAction::SetTrue)
                .help("CMake-only clean before building (CMake --clean-first)"))
            .arg(Arg::new("verbose")
                .long("verbose")
                .action(ArgAction::SetTrue)
                .help("CMake-only verbose build output (CMake --verbose)"))
            .arg(Arg::new("job_count")
                .long("jobs")
                .short('j')
                .help("Number of parallel jobs to run")
                .default_value("auto"))
            .arg(Arg::new("extra_args")
                .trailing_var_arg(true)
                .num_args(1..)
                .allow_hyphen_values(true)
                .help("Arguments to pass to the underlying build tool (e.g. MSBuild/Ninja)")
            )
        )
        .subcommand(clap::Command::new("init")
            .about("Initialize development toolchain files under the workspace")
            .arg(Arg::new("toolchain")
                .long("toolchain")
                .short('t')
                .help("Toolchain to initialize under the workspace")
                .value_parser(clap::builder::PossibleValuesParser::new(["cmake"]))
                .required(true)
            )
        )
        .subcommand(clap::Command::new("status")
            .about("Shows the status of the git repositories under the workspace")
            .arg(git_dir_arg)
        )
}

static CMAKE_TOOLCHAIN_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../CMake");

/// Recursively scans directories for git repositories
fn find_git_repositories(dirs: Vec<PathBuf>) -> Result<Vec<PathBuf>, CommandError> {
    let mut git_dirs = Vec::new();
    for dir in dirs {
        let mut stack = Vec::new();
        stack.push(dir);
        while let Some(dir) = stack.pop() {
            if dir.join(".git").is_dir() {
                git_dirs.push(dir);
                continue;
            }
            let read_dir = std::fs::read_dir(&dir).map_err(|e| CommandError::Runtime {
                message: format!("Failed to read directory {}: {}", dir.display(), e)
            })?;
            
            for entry in read_dir {
                let entry = entry.map_err(|e| CommandError::Runtime {
                    message: format!("Failed to get directory entry: {}", e)
                })?;
                let path = entry.path();
                if path.is_dir() && path.join(".git").is_dir() {
                    git_dirs.push(path);
                } else if path.is_dir() {
                    stack.push(path);
                }
            }
        }
    }
    Ok(git_dirs)
}

pub struct DevPullCommand {}

impl DevPullCommand {
    fn run_pull(&self, dirs: Vec<PathBuf>) -> CommandResult {
        // Scan module folder for git repositories and run "git pull" on them
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message("Scanning for git repositories...");
        let git_dirs = find_git_repositories(dirs)?;
        
        let total_count = git_dirs.len();
        let completed_count = Mutex::new(0usize);
        
        pb.set_message(format!("Pulling (0/{})...", total_count));
        
        // Mutex locked mutable output map
        let output_map_locked = Mutex::new(HashMap::<PathBuf, (String, String, bool, Option<String>)>::new());
        
        git_dirs.par_iter().for_each(|path| {
            // Get current branch
            let branch = std::process::Command::new("git")
                .arg("rev-parse")
                .arg("--abbrev-ref")
                .arg("HEAD")
                .current_dir(&path)
                .output()
                .expect("Failed to run git rev-parse --abbrev-ref HEAD");
            let branch_name = if branch.status.success() {
                String::from_utf8_lossy(&branch.stdout).trim().to_string()
            } else {
                "unknown".to_string()
            };
            
            // Pull
            let output = std::process::Command::new("git")
                .arg("pull")
                .arg("--autostash")
                .current_dir(&path)
                .output()
                .expect("Failed to run git pull");
            
            let pull_success = output.status.success();
            let pull_output = String::from_utf8_lossy(&output.stdout).to_string();
            let pull_error = if !pull_success {
                Some(String::from_utf8_lossy(&output.stderr).to_string())
            } else {
                None
            };
            
            // Check if anything was actually pulled (not "Already up to date" or "is up to date")
            let has_updates = pull_success && 
                !pull_output.contains("Already up to date") && 
                !pull_output.contains("is up to date");
            
            if pull_success {
                // Submodule update recursive
                let _ = std::process::Command::new("git")
                    .arg("submodule")
                    .arg("update")
                    .arg("--init")
                    .arg("--recursive")
                    .current_dir(&path)
                    .status()
                    .expect("Failed to run git submodule update");
            }
            
            let mut output_map = output_map_locked.lock().unwrap();
            output_map.insert(path.clone(), (branch_name, pull_output, has_updates, pull_error));
            
            // Update progress counter
            let mut count = completed_count.lock().unwrap();
            *count += 1;
            pb.set_message(format!("Pulling ({}/{})...", *count, total_count));
        });
        
        pb.finish_and_clear();

        // Sort: repos with updates first, then up-to-date, then errors
        let mut repos: Vec<_> = output_map_locked.into_inner().unwrap().into_iter().collect();
        repos.sort_by_key(|(_, (_, _, has_updates, error))| {
            if error.is_some() {
                2 // Errors last
            } else if *has_updates {
                1 // Updates in the middle
            } else {
                0 // Already up to date is first
            }
        });

        for (path, (branch, output, has_updates, error)) in repos {
            if let Some(err) = error {
                println!(
                    "{} ({}):",
                    path.display().to_string().red().bold(),
                    branch.cyan()
                );
                println!("  {}", err.trim());
                println!();
            } else if has_updates {
                println!(
                    "{} ({}):",
                    path.display().to_string().green().bold(),
                    branch.cyan()
                );
                // Show only meaningful lines from git pull output
                for line in output.lines() {
                    let line = line.trim();
                    if !line.is_empty() && !line.starts_with("From ") {
                        println!("  {}", line);
                    }
                }
                println!();
            } else {
                println!("{}", format!(
                    "{} ({}) {}",
                    path.display().to_string().green(),
                    branch.cyan(),
                    "(already up to date)").dimmed()
                );
            }
        }
        Ok(())
    }
}

impl Command for DevPullCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("dev") {
            return subcommand.subcommand_matches("pull");
        }
        None
    }

    fn run(&self, _workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let dirs: Vec<&String> = args.get_many::<String>("dir").unwrap_or_default().collect();
        let dirs: Vec<PathBuf> = dirs.iter().map(|s| PathBuf::from(s)).collect();
        self.run_pull(dirs)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}

pub struct DevGenCommand {}

impl DevGenCommand {
    fn run_gen(&self, lang_tool: LangTool, project_folder: &String, plugin_dirs: Option<String>, extra_args: Vec<String>) -> CommandResult {
        // Only cpp/cmake is supported for now
        if lang_tool != LangTool::CppCMake {
            return Err(InvalidArgument { message: format!("Unsupported language/tool: {}", lang_tool) });
        }
        let mut cmake_args = vec!["-S", "Toolchain/CMake", "-B", project_folder, "-DNOS_INVOKED_FROM_NOSMAN=ON"];

        let mut formatted_args = Vec::new(); // holds the actual Strings
        if let Some(val) = plugin_dirs {
            if val.is_empty() {
                cmake_args.push("-U MODULE_DIRS");
            } else {
                // store formatted string so it lives long enough
                formatted_args.push(format!("-DMODULE_DIRS={}", val));
                // push a reference to it
                cmake_args.push(formatted_args.last().unwrap().as_str());
            }
        }

        for arg in extra_args.iter() {
            cmake_args.push(arg);
        }
        let mut cmd = std::process::Command::new("cmake");
        let cmd_args_str = cmake_args.iter().map(|s| s.as_ref()).collect::<Vec<&OsStr>>().join(OsStr::new(" "));
        println!("{}: {:?}", "Running cmake with".green(), cmd_args_str);
        let status = cmd
            .args(&cmake_args)
            .status();
        if !status.is_ok() {
            return Err(CommandError::Runtime { message: format!("Error during running '{:?}'. See output.", cmake_args)});
        }
        Ok(())
    }
}

impl Command for DevGenCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("dev") {
            return subcommand.subcommand_matches("gen");
        }
        None
    }

    fn run(&self, _workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let lang_tool = args.get_one::<String>("language/tool").unwrap();
        let lang_tool = LangTool::from_str(lang_tool.as_str())
            .ok_or(InvalidArgument { message: format!("Unsupported language/tool: {}", lang_tool) })?;
        let project_folder = args.get_one::<String>("project_folder").unwrap();
        let extra_args = args
            .get_many::<String>("extra_args")
            .map(|vals| vals.cloned().collect())
            .unwrap_or_default();
        let plugin_dirs: Option<String>;
        match args.get_one::<String>("plugin_dirs"){
            None => {
                plugin_dirs = None;
            }
            Some(p) if p == "*" => {
                plugin_dirs = Some(String::from(""));
            }
            Some(p) => {
                plugin_dirs = Some(p.clone());
            }
        }
        self.run_gen(lang_tool, project_folder, plugin_dirs, extra_args)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}

pub struct DevStatusCommand {}

impl DevStatusCommand {
    fn run_status(&self, dirs: Vec<PathBuf>) -> CommandResult {
        // Scan module folder for git repositories and run "git pull" on them
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message("Scanning for git repositories...");
        let git_dirs = find_git_repositories(dirs)?;
        pb.set_message("Scanning...");
        // Mutex locked mutable output map
        let output_map_locked = Mutex::new(HashMap::<PathBuf, (String, String, bool)>::new());
        git_dirs.par_iter().for_each(|path| {
            // Get current branch
            let branch = std::process::Command::new("git")
                .arg("rev-parse")
                .arg("--abbrev-ref")
                .arg("HEAD")
                .current_dir(&path)
                .output()
                .expect("Failed to run git rev-parse --abbrev-ref HEAD");
            let branch_name = if branch.status.success() {
                String::from_utf8_lossy(&branch.stdout).trim().to_string()
            } else {
                "unknown".to_string()
            };

            // Run git status
            let status = std::process::Command::new("git")
                .arg("status")
                .arg("--porcelain")
                .current_dir(&path)
                .output()
                .expect("Failed to run git status");
            let status_str = String::from_utf8_lossy(&status.stdout).to_string();
            let has_changes = !status_str.trim().is_empty();

            let mut output_map = output_map_locked.lock().unwrap();
            output_map.insert(path.clone(), (branch_name, status_str, has_changes));
        });
        pb.finish_and_clear();

        // Sort: unchanged repos first, then changed
        let mut repos: Vec<_> = output_map_locked.into_inner().unwrap().into_iter().collect();
        repos.sort_by_key(|(_, (_, _, has_changes))| *has_changes);

        for (path, (branch, status, has_changes)) in repos {
            if has_changes {
                println!(
                    "{} ({}):",
                    path.display().to_string().green().bold(),
                    branch.cyan()
                );

                for line in status.lines() {
                    let line = line.trim_end();
                    println!("{}", line);
                }
                println!();
            } else {
                println!("{}", format!(
                    "{} ({}) {}",
                    path.display().to_string().green(),
                    branch.cyan(),
                    "(no changes)").dimmed()
                );
            }
        }
        Ok(())
    }
}

impl Command for DevStatusCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("dev") {
            return subcommand.subcommand_matches("status");
        }
        None
    }

    fn run(&self, _workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let dirs: Vec<&String> = args.get_many::<String>("dir").unwrap_or_default().collect();
        let dirs: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();
        self.run_status(dirs)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}

pub struct DevBuildCommand {}

impl DevBuildCommand {
    fn run_build(
        &self,
        lang_tool: LangTool,
        project_folder: &String,
        jobs: &String,
        cmake_config: Option<&String>,
        cmake_target: Option<&String>,
        clean_first: bool,
        verbose: bool,
        extra_args: Vec<String>
    ) -> CommandResult {
        // Only cpp/cmake is supported for now
        if lang_tool != LangTool::CppCMake {
            return Err(InvalidArgument { message: format!("Unsupported language/tool: {}", lang_tool) });
        }
        let job_count: usize = if jobs == "auto" {
            std::thread::available_parallelism()?.get()
        } else {
            jobs.parse::<usize>().map_err(|_| InvalidArgument { message: format!("Invalid job count: {}", jobs) })?
        };
        let job_count_str = job_count.to_string();
        match lang_tool {
            LangTool::CppCMake => {
                let mut build_args = vec!["--build".to_string(), project_folder.clone()];
                if let Some(config) = cmake_config {
                    build_args.push("--config".to_string());
                    build_args.push(config.clone());
                }
                if let Some(target) = cmake_target {
                    build_args.push("--target".to_string());
                    build_args.push(target.clone());
                }
                if clean_first {
                    build_args.push("--clean-first".to_string());
                }
                if verbose {
                    build_args.push("--verbose".to_string());
                }
                // If windows, and we use msbuild, cmake.exe --build --parallel <n_msbuild> -- /p:CL_MPcount=<n_cl>
                if cfg!(windows) {
                    build_args.push("--parallel".to_string());
                    build_args.push((job_count / 2).to_string());
                    build_args.push("--".to_string());
                    // TODO: Check if the compiler is Visual Studio
                    build_args.push(format!("/p:CL_MPCount={}", job_count_str));
                } else {
                    build_args.push("--parallel".to_string());
                    build_args.push(job_count_str);
                }
                for arg in extra_args.iter() {
                    build_args.push(arg.clone());
                }
                let mut cmd = std::process::Command::new("cmake");
                let cmd_args_str = build_args.iter().map(|s| s.as_ref()).collect::<Vec<&std::ffi::OsStr>>().join(std::ffi::OsStr::new(" "));
                println!("{}: {:?}", "Running cmake build with".green(), cmd_args_str);
                let status = cmd
                    .args(&build_args)
                    .status();
                if !status.is_ok() || !status.unwrap().success() {
                    return Err(CommandError::Runtime { message: format!("Error during running '{:?}'. See output.", build_args)});
                }
                Ok(())
            }
        }
    }
}

impl Command for DevBuildCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a clap::ArgMatches) -> Option<&'a clap::ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("dev") {
            return subcommand.subcommand_matches("build");
        }
        None
    }

    fn run(&self, _workspace: &mut Workspace, _command_name: Option<&str>, args: &clap::ArgMatches) -> CommandResult {
        let lang_tool = args.get_one::<String>("language/tool").unwrap();
        let lang_tool = LangTool::from_str(lang_tool.as_str())
            .ok_or(InvalidArgument { message: format!("Unsupported language/tool: {}", lang_tool) })?;
        let project_folder = args.get_one::<String>("project_folder").unwrap();
        let jobs = args.get_one::<String>("job_count").unwrap();
        let cmake_config = args.get_one::<String>("config");
        let cmake_target = args.get_one::<String>("target");
        let clean_first = args.get_flag("clean_first");
        let verbose = args.get_flag("verbose");
        let extra_args = args
            .get_many::<String>("extra_args")
            .map(|vals| vals.cloned().collect())
            .unwrap_or_default();
        self.run_build(
            lang_tool,
            project_folder,
            jobs,
            cmake_config,
            cmake_target,
            clean_first,
            verbose,
            extra_args
        )
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}

pub struct DevInitCommand {}

impl DevInitCommand {
    pub fn run_init(&self, workspace: &Workspace, toolchain: &str) -> CommandResult {
        match toolchain {
            "cmake" => {
                let toolchain_root = workspace.root.join("Toolchain");
                let toolchain_dir = toolchain_root.join("CMake");
                if toolchain_dir.exists() {
                    fs::remove_dir_all(&toolchain_dir)?;
                }
                fs::create_dir_all(&toolchain_root)?;
                copy_include_dir_recursive(&CMAKE_TOOLCHAIN_DIR, &toolchain_dir, None)?;
                println!(
                    "{}",
                    format!("Initialized CMake toolchain under {}", toolchain_dir.display()).green()
                );
                Ok(())
            }
            _ => Err(InvalidArgument {
                message: format!("Unsupported toolchain: {}", toolchain),
            }),
        }
    }
}

impl Command for DevInitCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("dev") {
            return subcommand.subcommand_matches("init");
        }
        None
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let toolchain = args.get_one::<String>("toolchain").unwrap();
        self.run_init(workspace, toolchain)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}

