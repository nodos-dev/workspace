extern crate clap;

use std::cell::RefCell;
use std::collections::HashMap;
use clap::{Arg, ArgAction, Command};

use std::error::Error;
use std::mem;
use clap::builder::StyledStr;
use colored::Colorize;
use sysinfo::System;
use crate::nosman::{command, constants};
use crate::nosman::command::sample;
use crate::nosman::workspace::Workspace;

mod nosman;

fn print_error(e: &dyn Error) {
    eprintln!("{}", format!("Error: {}", e).as_str().red());
    let mut cause = e.source();
    while let Some(e) = cause {
        eprintln!("{}", format!("Caused by: {}", e).as_str().red());
        cause = e.source();
    }
}

fn launched_from_file_explorer() -> bool {
    let mut sys = System::new_all();
    sys.refresh_all();
    if let Ok(pid) = sysinfo::get_current_pid() {
        if let Some(process) = sys.process(pid) {
            if let Some(parent_pid) = process.parent() {
                if let Some(parent_process) = sys.process(parent_pid) {
                    #[cfg(target_os = "windows")]
                    return parent_process.name().eq_ignore_ascii_case("explorer.exe");
                    #[cfg(target_os = "macos")]
                    return parent_process.name().eq_ignore_ascii_case("finder");
                    #[cfg(target_os = "linux")]
                    return ["nautilus", "dolphin", "nemo", "thunar"]
                        .iter()
                        .any(|&name| parent_process.name().eq_ignore_ascii_case(name));
                }
            }
        }
    }
    false
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        // Get parent process name. If it is a file explorer, open Nodos
        if launched_from_file_explorer() {
            let workspace_dir = std::env::current_dir().expect("Unable to access current working directory.");
            nosman::command::launch::launch_nodos(&workspace_dir, false);
            return;
        }
    }

    let exe_path = std::env::current_exe().expect("Unable to get current executable path");
    let stem = exe_path.file_stem().expect("Unable to get executable name").to_str().expect("Unable to convert executable name to string");
    let mut cli = Command::new(stem.to_string())
        .disable_help_flag(true)
        .version(env!("VERGEN_BUILD_SEMVER"))
        .about("Nodos Package Manager")
        .arg(Arg::new("workspace")
            .help("Directory to the workspace")
            .short('w')
            .long("workspace")
            .default_value(".")
        )
        .arg(Arg::new("silently_agree_eula")
            .help("Agrees to Nodos EULA. If multiple engines are installed, it will agree to all of their EULAs.")
            .long("silently-agree-eula")
            .action(ArgAction::SetTrue)
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("help")
            .short('h')
            .long("help")
            .help("Prints help information about a command")
        );
    cli = command::register_cli(cli);

    let workspace_dir = get_workspace_dir_from_cmd(&cli);
    let workspace = Workspace::from_root(&workspace_dir);

    let mut subcommand_helps: HashMap<String, StyledStr> = HashMap::new();
    for subcommand in cli.get_subcommands_mut() {
        let moved = mem::take(subcommand);
        // Re-enable help flag for subcommands
        *subcommand = moved.disable_help_flag(false);
        subcommand_helps.insert(subcommand.get_name().to_string(), subcommand.render_help());
    }

    // Add commands from extensions
    if workspace.ready() {
        cli = nosman::extensions::add_extensions(&workspace, cli);
    }

    let help_str = cli.render_help();
    let matches = cli.get_matches();

    // If contains --silently-agree-eula, agree to EULAs
    if let Some(agree_eula) = matches.get_one::<bool>("silently_agree_eula") {
        if *agree_eula {
            workspace.exit_if_required_but_not_found(true);
            nosman::eula::silently_agree_eulas(&workspace.root);
            return;
        }
    }

    // If -h comes first, print help and exit
    if matches.contains_id("help") {
        // If help is called without a subcommand, print the help string
        let subcommand_name = matches.get_one::<String>("help");
        if subcommand_name.is_none() {
            println!("{}", help_str.ansi());
            std::process::exit(0);
        }
        // If help is called with a subcommand, print the help string for that subcommand
        let subcommand_name = subcommand_name.unwrap();
        if let Some(help) = subcommand_helps.get(subcommand_name) {
            println!("{}", help.ansi());
            std::process::exit(0);
        }
    }

    let mut matched = false;
    let workspace_ref = RefCell::new(workspace);
    for command in nosman::command::commands().iter() {
        let match_res = command.matched_args(&workspace_ref.borrow(), &matches);
        match match_res {
            Some(matched_args) => {
                workspace_ref.borrow().exit_if_required_but_not_found((*command).needs_workspace());
                match (*command).run(&mut workspace_ref.borrow_mut(), matches.subcommand_name(), matched_args) {
                    Ok(_) => {
                        // nothing
                    },
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                };
                matched = true;
                break;
            }
            None => continue,
        };
    }

    if !matched {
        println!("{}", help_str.ansi());
        std::process::exit(1);
    }
}

fn get_workspace_dir_from_cmd(cmd: &Command) -> std::path::PathBuf {
    let mut wcmd = cmd.clone();
    wcmd = wcmd.subcommand(Command::new("help")) // trick because we can't get workspace dir without parsing everything.
        .disable_help_subcommand(true)
        .ignore_errors(true);
    let matches = wcmd.get_matches();
    // TODO: Try to get --workspace option without having to clone command and parse all args.
    std::path::PathBuf::from(matches.get_one::<String>("workspace").unwrap_or(&".".to_string()))
}

