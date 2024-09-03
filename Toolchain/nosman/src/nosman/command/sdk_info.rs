use clap::{ArgMatches};
use serde::{Deserialize, Serialize};

use crate::nosman::command::{Command, CommandResult};
use crate::nosman::path::get_default_engines_dir;
use crate::nosman::workspace;
use crate::nosman::index::SemVer;

pub struct SdkInfoCommand {
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct SdkInfo {
    version: String,
    path: String,
}

impl SdkInfoCommand {
    fn run_get_sdk_info(&self,  requested_version: &str, sdk_type: &str) -> CommandResult {
        // Search ./Engine directory under workspace dir and find the version.json with bin/ include/ folders in it
        let workspace_dir = workspace::current_root().unwrap();
        let engines_dir = get_default_engines_dir(&workspace_dir);
        if !engines_dir.exists() {
            return Err(crate::nosman::command::CommandError::InvalidArgumentError { message: "No Engine directory found in workspace".to_string() });
        }

		// Determine the correct version key based on sdk_type
		let version_key = match sdk_type {
			"engine" => "version",
			"plugin" => "plugin_sdk_version",
			"subsystem" => "subsystem_sdk_version",
			"process" => "process_sdk_version",
			_ => return Err(crate::nosman::command::CommandError::InvalidArgumentError { message: format!("Invalid SDK type: {}", sdk_type) }),
		};

		let requested_sem_ver = match SemVer::parse_from_string(requested_version) {
			Some(semver) => semver,
			None => return Err(crate::nosman::command::CommandError::InvalidArgumentError { message: format!("Invalid version: {}", requested_version) }),
		};

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
            let info_str = std::fs::read_to_string(info_file).expect("Failed to read SDK info file");
            let info_json: serde_json::Value = serde_json::from_str(&info_str).expect("Failed to parse SDK info file");
            let version = match info_json.get(version_key)
			{
				Some(version) => version.as_str(),
				None=> continue,
			}.expect("Version field is not a string");
            let sdk_sem_ver = match SemVer::parse_from_string(version)
			{
				Some(semver) => semver,
				None => continue,
			};
			if sdk_sem_ver.satisfies_requested_version(&requested_sem_ver) {
                let bin_dir = sdk_dir.join("bin");
                let include_dir = sdk_dir.join("include");
                if bin_dir.exists() && include_dir.exists() {
                    let path_str = dunce::canonicalize(workspace_dir.join(sdk_dir).canonicalize()
                        .expect("Failed to canonicalize SDK directory"))
                        .expect("Failed to canonicalize SDK directory").to_str()
                        .expect("Failed to convert path to string").to_string();
                    let sdk_info = SdkInfo {
                        version: version.to_string(),
                        path: path_str,
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
		let sdk_type_opt = args.get_one::<String>("sdk-type").map(|s| s.as_str());
		let sdk_type = sdk_type_opt.unwrap_or("engine");
        self.run_get_sdk_info(version, sdk_type)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
