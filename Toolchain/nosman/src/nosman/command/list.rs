use chrono::DateTime;
use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use inquire::{MultiSelect};
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::index::SemVer;
use crate::nosman::workspace::{Workspace};

pub struct ListCommand {}

impl ListCommand {
    fn run_list(&self, workspace: &mut Workspace, mut local: bool, mut remote: bool, opt_package_name: Option<&String>) -> CommandResult {
        if !local && !remote {
            // Select
            let selection = MultiSelect::new("What do you want to list?", vec!["Local packages", "Remote packages"])
                .prompt();
            let selection = selection.map_err(|e| crate::nosman::command::CommandError::Runtime { message: format!("Failed to prompt user: {}", e) })?;
            for sel in selection {
                match sel {
                    "Local packages" => local = true,
                    "Remote packages" => remote = true,
                    _ => {}
                }
            }
            if !local && !remote {
                println!("{}", "Nothing selected".to_string().yellow());
            }
        }

        if let Some(package_name) = opt_package_name {
            if local {
                println!("{}", format!("Local versions of {}", package_name).green());
                let mut local_versions = Vec::new();
                for (name, ver_map) in &workspace.packages {
                    for (version, module) in ver_map {
                        if name == package_name {
                            local_versions.push((name.clone(), version.clone(), module.clone()));
                        }
                    }
                }
                local_versions.sort_by(|a, b| {
                    let a_version = SemVer::parse_from_str(a.1.as_str());
                    let b_version = SemVer::parse_from_str(b.1.as_str());
                    a_version.cmp(&b_version)
                });
                for (_name, version, module) in local_versions {
                    println!("  {} ({})", format!("{}", version).green(), module.get_package_root().display());
                }
            }
            if remote {
                workspace.set_output_mode(crate::nosman::workspace::OutputMode::Silent);
                workspace.fetch_package_releases(package_name);
                println!("{}", "Remote versions".green());
                if let Some(releases) = workspace.index_cache.packages.get(package_name) {
                    for release_entry in &releases.1 {
                        let mut out_str = String::new();
                        out_str.push_str(format!("{}", release_entry.version.green()).as_str());
                        if let Some(ref platform) = release_entry.platform {
                            out_str.push_str(&format!(" ({})", platform.yellow()));
                        }
                        if let Some(ref plugin_api_version) = release_entry.plugin_api_version {
                            out_str.push_str(&format!(" (Nodos API version: {})", plugin_api_version.to_string().yellow()));
                        }
                        if let Some( ref subsystem_api_version) = release_entry.subsystem_api_version {
                            out_str.push_str(&format!(" (Nodos API version: {})", subsystem_api_version.to_string().yellow()));
                        }
                        if let Some(date) = &release_entry.release_date {
                            out_str.push_str(&format!(" ({})", DateTime::parse_from_rfc3339(date)
                                .map(|dt| dt.format("%d %b %Y").to_string())
                                .unwrap_or_else(|_| "Invalid date".to_string()).yellow()));
                        }
                        println!("  {}", out_str);
                    }
                }
            }
        } else {
            if local {
                println!("{}", "Local packages".green());
                let mut installed_modules_alphabetical = Vec::new();
                for (name, ver_map) in &workspace.packages {
                    for (version, module) in ver_map {
                        installed_modules_alphabetical.push((name.clone(), version.clone(), module.clone()));
                    }
                }
                installed_modules_alphabetical.sort_by(|a, b| a.0.cmp(&b.0));
                for (name, version, module) in installed_modules_alphabetical {
                    println!("  {} ({})", format!("{} ({})", name.green(), version.yellow()), module.get_package_root().display());
                }
            }
            if remote {
                workspace.set_output_mode(crate::nosman::workspace::OutputMode::Silent);
                let mut latest = workspace.fetch_latest_versions();
                println!("{}", "Remote packages".green());
                latest.sort_by(|a, b| a.0.cmp(&b.0));
                for (name, entry) in latest {
                    println!("  {} (latest: {})", name.to_string().green(), entry.version.to_string().yellow());
                }
            }
        }
        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("list")
        .about("List packages")
        .arg(Arg::new("local")
            .action(ArgAction::SetTrue)
            .help("List local packages")
            .long("local")
            .alias("installed")
            .num_args(0)
            .required(false)
            .group("list_type")
        )
        .arg(Arg::new("remote")
            .action(ArgAction::SetTrue)
            .help("List remote packages")
            .long("remote")
            .num_args(0)
            .required(false)
            .group("list_type")
        )
        .arg(Arg::new("package_name")
            .help("Name of the package to list remote/local packages of")
            .long("package-name")
            .short('p')
            .required(false)
        )
}

impl Command for ListCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("list")
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let local = args.get_one::<bool>("local").unwrap();
        let remote = args.get_one::<bool>("remote").unwrap();
        let opt_package_name = args.get_one::<String>("package_name");
        self.run_list(workspace, *local, *remote, opt_package_name)
    }
}
