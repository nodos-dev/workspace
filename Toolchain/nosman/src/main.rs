extern crate clap;
use clap::{Arg, command, value_parser, ArgAction, Command};

use std::error::Error;
use colored::Colorize;

mod nosman;

fn print_error(e: &dyn Error) {
    eprintln!("{}", format!("Error: {}", e).as_str().red());
    let mut cause = e.source();
    while let Some(e) = cause {
        eprintln!("{}", format!("Caused by: {}", e).as_str().red());
        cause = e.source();
    }
}

fn main() {
    let matches = Command::new("nosman")
        .version(env!("VERGEN_BUILD_SEMVER"))
        .about("Nodos Package Manager")
        .arg(Arg::new("workspace")
            .help("Directory to the workspace")
            .short('w')
            .long("workspace")
            .default_value(".")
        )
        .subcommand(Command::new("init")
            .about("Initialize a directory as a Nodos workspace.")
        )
        .subcommand(Command::new("deinit")
            .about("Deinitialize a Nodos workspace.")
        )
        .subcommand(Command::new("install")
            .about("Install a module")
            .arg(Arg::new("module").required(true))
            .arg(Arg::new("version").required(false).default_value("latest"))
            .arg(Arg::new("exact")
                .action(ArgAction::SetTrue)
                .help("If not set, version parameter will be interpreted as minimum required version within that minor/patch version. \
                If no version 'x' such that 'a.b <= x < a.(b+1)' is found among installed modules, latest such version will be installed. \
                If version is set to 'latest' or has no minor component, it will fail.")
                .long("exact")
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("prefix")
                .help("Folder path relative to out_dir. The module contents will be under this folder. By default, its '<module_name>-<version>'.")
                .long("prefix")
                .required(false)
            )
            .arg(Arg::new("out_dir")
                .help("The directory where the module will be installed")
                .default_value("./Module")
                .long("out-dir")
                .required(false)
            )
        )
        .subcommand(Command::new("remove")
            .about("Remove a module")
            .arg(Arg::new("module").required(true))
            .arg(Arg::new("version").required(true))
        )
        .subcommand(Command::new("rescan")
            .about("Rescan modules and update caches")
        )
        .subcommand(Command::new("list")
            .about("List installed modules")
        )
        .subcommand(Command::new("info")
            .about("Returns information about an installed module in JSON format. If no such module is installed, it will return an error.")
            .arg(Arg::new("module").required(true))
            .arg(Arg::new("version").required(true))
            .arg(Arg::new("relaxed")
                .action(ArgAction::SetTrue)
                .help("If set, version parameter will be interpreted as minimum required version within that minor/patch version. \
                It will return information about a version 'x' found among installed modules such that 'a.b <= x < a.(b+1)'.")
                .long("relaxed")
                .num_args(0)
                .required(false)
            )
        )
        .subcommand(Command::new("remote")
            .about("Manage remotes.")
            .subcommand(Command::new("add")
                .about("Add a remote")
                .arg(Arg::new("url").required(true))
            )
            .subcommand(Command::new("list")
                .about("List remotes")
            )
            .subcommand(Command::new("remove")
                .about("Remove a remote")
                .arg(Arg::new("url").required(true))
            )
        ).get_matches();

    let mut matched = false;
    for command in nosman::command::commands().iter() {
        match command.matched_args(&matches) {
            Some(command_args) => {
                nosman::workspace::set_current(std::path::PathBuf::from(matches.get_one::<String>("workspace").unwrap()));
                if (*command).needs_workspace() {
                    if !nosman::workspace::current().unwrap().join(".nosman").exists() {
                        eprintln!("No workspace found in {:?}", matches.get_one::<String>("workspace").unwrap());
                        std::process::exit(1);
                    }
                }
                match (*command).run(command_args) {
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
            },
            None => continue,
        };
    }

    if !matched {
        eprintln!("Invalid command.");
        std::process::exit(1);
    }

    std::process::exit(0);
}

