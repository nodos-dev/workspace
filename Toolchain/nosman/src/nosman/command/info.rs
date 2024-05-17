use clap::{ArgMatches};
use crate::nosman;

use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::index::SemVer;

use crate::nosman::workspace::{Workspace};

pub struct InfoCommand {
}

impl InfoCommand {
    fn run_get_info(&self, module_name: &str, version: &str, relaxed: bool) -> CommandResult {
        let workspace = Workspace::get()?;
        let module =  if relaxed {
            let semver_res = SemVer::parse_from_string(version);
            if semver_res.is_none() {
                return Err(CommandError::InvalidArgumentError { message: format!("Invalid semantic version: {}. Unable to use with --relaxed option.", version) });
            }
            let version_start = SemVer::parse_from_string(version).unwrap();
            if version_start.minor.is_none() {
                return Err(CommandError::InvalidArgumentError { message: "Please provide a minor version too!".to_string() });
            }
            let version_end = version_start.get_one_up();
            let res = workspace.get_latest_installed_module_within_range(module_name, &version_start, &version_end);
            if res.is_none() {
                return Err(CommandError::InvalidArgumentError { message: format!("No installed version in range [{}, {}) for module {}", version_start.to_string(), version_end.to_string(), module_name) });
            }
            res.unwrap()
        } else {
            let res = workspace.get_installed_module(module_name, version);
            if res.is_none() {
                return Err(CommandError::InvalidArgumentError { message: format!("Module {} version {} is not installed", module_name, version) });
            }
            res.unwrap()
        };
        let json_str = serde_json::to_string_pretty(&module).unwrap();
        println!("{}", json_str);
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
        let relaxed = args.get_one::<bool>("relaxed").unwrap();
        self.run_get_info(module_name, version, relaxed.clone())
    }
}
