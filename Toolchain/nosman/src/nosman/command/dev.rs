use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use indicatif::ProgressBar;
use rayon::prelude::*;
use CommandError::InvalidArgument;
use crate::nosman::command::{get_lang_tool_arg, Command, CommandError, CommandResult};
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
            .about("Generates project files for Nodos module development")
            .arg(get_lang_tool_arg())
            .arg(Arg::new("project_folder")
                .long("project-folder")
                .short('p')
                .help("Path to the project folder to generate files in")
                .default_value("Project"))
            .arg(Arg::new("module_dirs")
                .help("Module paths to generate if only one of them is wanted")
                .num_args(0..=1) // 0 or 1 argument allowed
                .value_name("module_dirs")
                .allow_hyphen_values(true))
            .arg(Arg::new("extra_args")
                .last(true)
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
            .arg(Arg::new("job_count")
                .long("jobs")
                .short('j')
                .help("Number of parallel jobs to run")
                .default_value("auto"))
            .arg(Arg::new("extra_args")
                .last(true)
                .help("Arguments to pass to the underlying tool when building project files")
            )
        )
        .subcommand(clap::Command::new("status")
            .about("Shows the status of the git repositories under the workspace")
            .arg(git_dir_arg)
        )
}

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
        pb.set_message("Pulling...");
        git_dirs.par_iter().for_each(|path| {
            // Get remote url
            let remote = std::process::Command::new("git")
                .arg("remote")
                .arg("get-url")
                .arg("origin")
                .current_dir(&path)
                .output()
                .expect("Failed to run git remote get-url origin");
            if !remote.status.success() {
                pb.println(format!("{}{}\n  {}", "Failed to get remote URL: ".red(), path.display(), String::from_utf8_lossy(&remote.stderr)));
                return;
            }
            let remote_url = String::from_utf8_lossy(&remote.stdout).trim().to_string();
            // Get current branch
            let branch = std::process::Command::new("git")
                .arg("rev-parse")
                .arg("--abbrev-ref")
                .arg("HEAD")
                .current_dir(&path)
                .output()
                .expect("Failed to run git rev-parse --abbrev-ref HEAD");
            if !branch.status.success() {
                pb.println(format!("{}{}\n  {}", "Failed to get current branch: ".red(), path.display(), String::from_utf8_lossy(&branch.stderr)));
                return;
            }
            let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();
            pb.println(format!("{}{} ({})", "Pulling: ".yellow(), path.display(), branch.cyan()));
            let output = std::process::Command::new("git")
                .arg("pull")
                .arg("--autostash")
                .current_dir(&path)
                .output()
                .expect("Failed to run git pull");
            if !output.status.success() {
                pb.println(format!("{}{} ({}) ({}):\n  {}", "Failed to pull: ".red(), path.display(), branch.cyan(), remote_url, String::from_utf8_lossy(&output.stderr)));
                return;
            }
            // Submodule update recursive
            let status = std::process::Command::new("git")
                .arg("submodule")
                .arg("update")
                .arg("--init")
                .arg("--recursive")
                .current_dir(&path)
                .status()
                .expect("Failed to run git submodule update");
            if !status.success() {
                pb.println(format!("{}{} ({}) ({}):\n  {}", "Failed to update submodules: ".red(), path.display(), branch.cyan(), remote_url, String::from_utf8_lossy(&output.stderr)));
                return;
            }
            pb.println(format!("{} ({}) ({}): {}", path.display().to_string().green(), branch.cyan(), remote_url, String::from_utf8_lossy(&output.stdout)));
        });
        pb.finish_and_clear();
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
    fn run_gen(&self, lang_tool: &String, project_folder: &String, module_dirs: Option<String>, extra_args: Vec<String>) -> CommandResult {
        // Only cpp/cmake is supported for now
        if lang_tool != "cpp/cmake" {
            return Err(InvalidArgument { message: format!("Unsupported language/tool: {}", lang_tool) });
        }
        let mut cmake_args = vec!["-S", "Toolchain/CMake", "-B", project_folder, "-DNOS_INVOKED_FROM_NOSMAN=ON"];

        let mut formatted_args = Vec::new(); // holds the actual Strings
        if let Some(val) = module_dirs{
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
        let project_folder = args.get_one::<String>("project_folder").unwrap();
        let mut extra_args = Vec::new();
        if let Some(args) = args.get_one::<String>("extra_args") {
            extra_args = args.split_whitespace().map(|s| s.to_string()).collect();
        }
        let path: Option<String>;
        match args.get_one::<String>("module_dirs"){
            None => {
                path = None;
            }
            Some(p) if p == "*" => {
                path = Some(String::from(""));
            }
            Some(p) => {
                path = Some(p.clone());
            }
        }
        self.run_gen(lang_tool, project_folder, path, extra_args)
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

        // Sort: changed repos first, then unchanged
        let mut repos: Vec<_> = output_map_locked.into_inner().unwrap().into_iter().collect();
        repos.sort_by_key(|(_, (_, _, has_changes))| !*has_changes);

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
    fn run_build(&self, lang_tool: &String, project_folder: &String, jobs: &String, extra_args: Vec<String>) -> CommandResult {
        // Only cpp/cmake is supported for now
        if lang_tool != "cpp/cmake" {
            return Err(InvalidArgument { message: format!("Unsupported language/tool: {}", lang_tool) });
        }
        let job_count: usize = if jobs == "auto" {
            std::thread::available_parallelism()?.get()
        } else {
            jobs.parse::<usize>().map_err(|_| InvalidArgument { message: format!("Invalid job count: {}", jobs) })?
        };
        let job_count_str = job_count.to_string();
        let mut build_args = vec!["--build".to_string(), project_folder.clone()];
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

impl Command for DevBuildCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a clap::ArgMatches) -> Option<&'a clap::ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("dev") {
            return subcommand.subcommand_matches("build");
        }
        None
    }

    fn run(&self, _workspace: &mut Workspace, _command_name: Option<&str>, args: &clap::ArgMatches) -> CommandResult {
        let lang_tool = args.get_one::<String>("language/tool").unwrap();
        let project_folder = args.get_one::<String>("project_folder").unwrap();
        let jobs = args.get_one::<String>("job_count").unwrap();
        let mut extra_args = Vec::new();
        if let Some(args) = args.get_one::<String>("extra_args") {
            extra_args = args.split_whitespace().map(|s| s.to_string()).collect();
        }
        self.run_build(lang_tool, project_folder, jobs, extra_args)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
