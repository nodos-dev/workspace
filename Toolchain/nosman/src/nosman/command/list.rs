use clap::{ArgMatches};
use colored::Colorize;
use inquire::{MultiSelect};
use crate::nosman::command::{Command, CommandResult};

use crate::nosman::workspace::{Workspace};

pub struct ListCommand {}

impl ListCommand {
    fn run_list(&self, workspace: &mut Workspace, mut installed: bool, mut remote: bool) -> CommandResult {
    
        if !installed && !remote {
            // Select
            let selection = MultiSelect::new("What do you want to list?", vec!["Installed modules", "Remote packages"])
                .prompt();
            let selection = selection.map_err(|e| crate::nosman::command::CommandError::Runtime { message: format!("Failed to prompt user: {}", e) })?;
            for sel in selection {
                match sel {
                    "Installed modules" => installed = true,
                    "Remote packages" => remote = true,
                    _ => {}
                }
            }
            if !installed && !remote {
                println!("{}", "Nothing selected".to_string().yellow());
            }
        }
        
        if installed {
            println!("{}", "Installed modules".green());
            let mut installed_modules_alphabetical = Vec::new();
            for (name, ver_map) in &workspace.installed_modules {
                for (version, module) in ver_map {
                    installed_modules_alphabetical.push((name.clone(), version.clone(), module.clone()));
                }
            }
            installed_modules_alphabetical.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, version, module) in installed_modules_alphabetical {                   
                println!("  {} ({})", format!("{}-{}", name, version).green(), module.get_module_dir().display());
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
        Ok(true)
    }
}

impl Command for ListCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("list")
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let installed = args.get_one::<bool>("installed").unwrap();
        let remote = args.get_one::<bool>("remote").unwrap();
        self.run_list(workspace, *installed, *remote)
    }
}
