use clap::{ArgMatches};

use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::extensions::{NosArg, NosArgAction, NosCommand, NosCommandDesc};
use crate::nosman::module::InstalledModule;
use crate::nosman::workspace::{Workspace};

pub struct Extension {
}

fn fill_command(desc: &NosCommandDesc, matches: &ArgMatches, outgoing: &mut NosCommand) {
    for desc_arg in &desc.args {
        let arg_name = desc_arg.name.as_str();
        if desc_arg.action == NosArgAction::Set {
            if let Some(arg_value) = matches.get_one::<String>(arg_name) {
                if !arg_value.is_empty() {
                    outgoing.args.push(NosArg {
                        name: arg_name.to_string(),
                        value: arg_value.clone(),
                    });
                }
            }
        } else {
            if let Some(arg_value) = matches.get_one::<bool>(arg_name) {
                outgoing.args.push(NosArg {
                    name: arg_name.to_string(),
                    value: arg_value.to_string(),
                });
            }
        }
    }
}

impl Extension {
    fn run(&self, module: &InstalledModule, command_name: &str, matches: &ArgMatches) -> CommandResult {
        let mut outgoing = NosCommand {
            name: command_name.to_string(),
            args: vec![],
            sub_command: None,
        };
        let mut found = None;
        for cmd in &module.commands {
            if cmd.name == command_name {
                found = Some(cmd);
                break;
            }
        }
        if found.is_none() {
            return Err(CommandError::InvalidArgumentError { message: format!("Command {} not found in module {}", command_name, module.manifest_path.display()) });
        }
        let desc = found.unwrap();
        fill_command(desc, matches, &mut outgoing);
        let mut stack = vec![(desc, matches)];
        while !stack.is_empty() {
            let (desc, matches) = stack.pop().unwrap();
            for desc_sub in &desc.sub_commands {
                if let Some(sub_command_matches) = matches.subcommand_matches(desc_sub.name.as_str()) {
                    let mut sub_outgoing = NosCommand {
                        name: desc_sub.name.clone(),
                        args: vec![],
                        sub_command: None,
                    };
                    fill_command(desc_sub, sub_command_matches, &mut sub_outgoing);
                    outgoing.sub_command = Some(Box::new(sub_outgoing));
                    stack.push((desc_sub, sub_command_matches));
                }
            }
        }
        module.run_command(&command_name, outgoing)
    }
    fn get_command(matches: &ArgMatches) -> Option<(&NosCommandDesc, &InstalledModule, &ArgMatches)> {
        let ws = Workspace::get().expect("Failed to get workspace");
        let latest_modules = ws.get_latest_installed_modules();
        for module in latest_modules {
            for command in &module.commands {
                if let Some(sub_matches) = matches.subcommand_matches(command.name.as_str()) {
                    return Some((command, module, sub_matches));
                }
            }
        }
        None
    }
    fn get_command_by_name(command_name: &str) -> Option<(&NosCommandDesc, &InstalledModule)> {
        let ws = Workspace::get().expect("Failed to get workspace");
        let latest_modules = ws.get_latest_installed_modules();
        for module in latest_modules {
            for command in &module.commands {
                if command.name == command_name {
                    return Some((&command, module));
                }
            }
        }
        None
    }
}

impl Command for Extension {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some((_command_desc, _module, matches)) = Self::get_command(&args) {
            return Some(matches);
        }
        None
    }

    fn run(&self, subcommand_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        if subcommand_name.is_none() {
            return Err(CommandError::InvalidArgumentError { message: "No subcommand provided".to_string() });
        }
        if let Some((command_desc, module)) = Self::get_command_by_name(subcommand_name.unwrap()) {
            return self.run(module, command_desc.name.as_str(), args);
        }
        Err(CommandError::InvalidArgumentError { message: "No command found".to_string() })
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
