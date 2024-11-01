use clap::{ArgMatches};
use colored::Colorize;
use crate::nosman::command::{Command, CommandError, CommandResult};

use crate::nosman::workspace::{RescanFlags, Workspace};

pub struct InfoCommand {
}

impl InfoCommand {
    fn run_get_info(&self, module_name: &str, version: &str, relaxed: bool, rescan_if_needed: bool) -> CommandResult {
        let workspace = Workspace::get()?;
        let module =  if relaxed {
            let res = workspace.get_latest_installed_module_for_version(module_name, version);
            if let Err(msg) = res {
                return Err(CommandError::InvalidArgumentError { message: msg });
            }
            res.unwrap()
        } else {
            let res = workspace.get_installed_module(module_name, version);
            if res.is_none() {
                if rescan_if_needed {
                    println!("{}", format!("Module {} version {} is not found. Rescanning...", module_name, version).yellow());
                    workspace.rescan(RescanFlags::all())?;
                    workspace.save()?;
                    return self.run_get_info(module_name, version, relaxed, false);
                }
                return Err(CommandError::InvalidArgumentError { message: format!("Module {} version {} is not installed", module_name, version) });
            }
            res.unwrap()
        };
        // Rescan if needed.
        if rescan_if_needed && !module.needs_rescan(&workspace) {
            println!("{}", format!("Index entry for module {} version {} is out of date. Rescanning...", module_name, version).yellow());
            workspace.rescan(RescanFlags::all())?;
            workspace.save()?;
            return self.run_get_info(module_name, version, relaxed, false);
        }
        // Convert paths to full paths:
        let mut m = module.clone();
        m.manifest_path = workspace.root.join(&module.manifest_path);
        for file in &mut m.type_schema_files {
            *file = workspace.root.join(file.clone());
        }
        if let Some(ref mut folder) = m.public_include_folder {
            *folder = workspace.root.join(folder.clone());
        }
        let json_str = serde_json::to_string_pretty(&m).unwrap();
        println!("{}", json_str);
        Ok(true)
    }
}

impl Command for InfoCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("info")
    }

    fn run(&self, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let module_name = args.get_one::<String>("module").unwrap();
        let version = args.get_one::<String>("version").unwrap();
        let relaxed = args.get_one::<bool>("relaxed").unwrap();
        self.run_get_info(module_name, version, relaxed.clone(), true)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
