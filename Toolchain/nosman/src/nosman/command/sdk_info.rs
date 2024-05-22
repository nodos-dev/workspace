use clap::{ArgMatches};
use serde::{Deserialize, Serialize};

use crate::nosman::command::{Command, CommandResult};
use crate::nosman::workspace;

pub struct SdkInfoCommand {
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct SdkInfo {
    version: String,
    path: String,
}

impl SdkInfoCommand {
    fn run_get_sdk_info(&self, requested_version: &str) -> CommandResult {
        // Search ./Engine directory under workspace dir and find the version.json with bin/ include/ folders in it
        let workspace_dir = workspace::current().unwrap();
        let engines_dir = workspace_dir.join("Engine");
        if !engines_dir.exists() {
            return Err(crate::nosman::command::CommandError::InvalidArgumentError { message: "No Engine directory found in workspace".to_string() });
        }
        // For each folder in engines_dir, check if it has SDK/version.json
        for entry in std::fs::read_dir(engines_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let sdk_dir = path.join("SDK");
            if !sdk_dir.exists() {
                continue;
            }
            let info_file = sdk_dir.join("info.json");
            if !info_file.exists() {
                continue;
            }
            let info_str = std::fs::read_to_string(info_file).unwrap();
            let info_json: serde_json::Value = serde_json::from_str(&info_str).unwrap();
            let version = info_json.get("version").unwrap().as_str().unwrap();
            if version == requested_version {
                let bin_dir = sdk_dir.join("bin");
                let include_dir = sdk_dir.join("include");
                if bin_dir.exists() && include_dir.exists() {
                    let sdk_info = SdkInfo {
                        version: version.to_string(),
                        path: workspace_dir.join(sdk_dir).canonicalize().unwrap().to_str().unwrap().to_string(),
                    };
                    let json_str = serde_json::to_string_pretty(&sdk_info).unwrap();
                    println!("{}", json_str);
                    return Ok(true);
                }
            }
        }
        return Err(crate::nosman::command::CommandError::InvalidArgumentError { message: format!("No SDK found for version {}", requested_version) });
    }
}

impl Command for SdkInfoCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        return args.subcommand_matches("sdk-info");
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let version = args.get_one::<String>("version").unwrap();
        self.run_get_sdk_info(version)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
