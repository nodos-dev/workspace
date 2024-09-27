use clap::{ArgMatches};
use serde::{Deserialize, Serialize};

use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::path::get_default_engines_dir;
use crate::nosman::workspace;
use crate::nosman::index::SemVer;

pub struct SdkInfoCommand {
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SdkInfo {
    pub version: String,
    process_sdk_version: String,
    plugin_sdk_version: String,
    subsystem_sdk_version: String,
    path: String,
}

pub fn get_engine_sdk_infos() -> Result<Vec<SdkInfo>, CommandError> {
    let workspace_dir = workspace::current_root().unwrap();
    let engines_dir = get_default_engines_dir(&workspace_dir);
    if !engines_dir.exists() {
        return Err(CommandError::InvalidArgumentError { message: "No Engine directory found in workspace".to_string() });
    }

    let mut result = Vec::new();
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
        let version = info_json.get("version").expect("Version field not found").as_str().expect("Version field is not a string");
        let process_sdk_version = info_json.get("process_sdk_version").expect("process_sdk_version field not found").as_str().expect("process_sdk_version field is not a string");
        let plugin_sdk_version = info_json.get("plugin_sdk_version").expect("plugin_sdk_version field not found").as_str().expect("plugin_sdk_version field is not a string");
        let subsystem_sdk_version = info_json.get("subsystem_sdk_version").expect("subsystem_sdk_version field not found").as_str().expect("subsystem_sdk_version field is not a string");
        let bin_dir = sdk_dir.join("bin");
        let include_dir = sdk_dir.join("include");
        if bin_dir.exists() && include_dir.exists() {
            let path_str = dunce::canonicalize(dunce::canonicalize(sdk_dir)
                .expect("Failed to canonicalize SDK directory"))
                .expect("Failed to canonicalize SDK directory").to_str()
                .expect("Failed to convert path to string").to_string();
            let sdk_info = SdkInfo {
                version: version.to_string(),
                process_sdk_version: process_sdk_version.to_string(),
                plugin_sdk_version: plugin_sdk_version.to_string(),
                subsystem_sdk_version: subsystem_sdk_version.to_string(),
                path: path_str,
            };
            result.push(sdk_info);
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct SdkInfoOutput {
    version: String,
    path: String,
}

impl SdkInfoCommand {
    fn run_get_sdk_info(&self,  requested_version: &str, sdk_type: &str) -> CommandResult {
        // Search ./Engine directory under workspace dir and find the version.json with bin/ include/ folders in it
        let mut engines = get_engine_sdk_infos()?;

        // Sort the engines by version, latest first
        engines.sort_by(|a, b| {
            let a_sem_ver = SemVer::parse_from_string(&a.version).expect("Failed to parse SDK version");
            let b_sem_ver = SemVer::parse_from_string(&b.version).expect("Failed to parse SDK version");
            b_sem_ver.cmp(&a_sem_ver)
        });

        let requested_sem_ver = match SemVer::parse_from_string(requested_version) {
            Some(semver) => semver,
            None => return Err(CommandError::InvalidArgumentError { message: format!("Invalid version: {}", requested_version) }),
        };

        let mut found_sdk_info: Option<SdkInfo> = None;
        // Determine the correct version key based on sdk_type
        for sdk_info in engines {
            let sdk_sem_ver = SemVer::parse_from_string(&sdk_info.version).expect("Failed to parse SDK version");
            if sdk_sem_ver.satisfies_requested_version(&requested_sem_ver) {
                found_sdk_info = Some(sdk_info);
                break;
            }
        }
        if let Some(info) = found_sdk_info {
            let version = match sdk_type {
                "engine" => &info.version,
                "plugin" => &info.plugin_sdk_version,
                "subsystem" => &info.subsystem_sdk_version,
                "process" => &info.process_sdk_version,
                _ => return Err(CommandError::InvalidArgumentError { message: format!("Invalid SDK type: {}", sdk_type) }),
            };
            let output = SdkInfoOutput {
                version: version.to_string(),
                path: info.path,
            };
            println!("{}", serde_json::to_string_pretty(&output).expect("Failed to serialize SDK info"));
            return Ok(true);
        }

        Err(CommandError::InvalidArgumentError { message: format!("No SDK found for version {}", requested_version) })
    }
}

impl Command for SdkInfoCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("sdk-info")
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
