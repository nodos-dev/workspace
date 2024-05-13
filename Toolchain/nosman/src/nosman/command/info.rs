use clap::{ArgMatches};

use crate::nosman::command::{Command, CommandError, CommandResult};

use crate::nosman::workspace::{Workspace};

pub struct InfoCommand {
}

impl InfoCommand {
    fn run_get_info(&self, module_name: &str, version: &str, property: &str) -> CommandResult {
        let workspace = Workspace::get();
        let res = workspace.get_installed_module(module_name, version);
        if res.is_none() {
            return Err(CommandError::InvalidArgumentError { message: format!("Module {} version {} is not installed", module_name, version) });
        }
        let module = res.unwrap();
        match property {
            "name" => println!("{}", module.info.id.name),
            "version" => println!("{}", module.info.id.version),
            "include_folder" => println!("{}", if module.public_include_folder.is_some() { module.public_include_folder.as_ref().unwrap().to_str().unwrap() } else { "" }),
            "config_path" => println!("{}", module.config_path.to_str().unwrap()),
            _ => return Err(CommandError::InvalidArgumentError { message: format!("Property {} is not valid", property) }),
        }
        Ok(true)
    }
}

impl Command for InfoCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        return args.subcommand_matches("info");
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let module_name = args.get_one::<String>("module").unwrap();
        let version = args.get_one::<String>("version").unwrap();
        let property = args.get_one::<String>("property").unwrap();
        self.run_get_info(module_name, version, property)
    }
}
