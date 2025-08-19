use clap::{Arg, ArgMatches};
use serde::{Deserialize, Serialize};
use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::common;
use crate::nosman::path::get_default_engines_dir;
use crate::nosman::index::SemVer;
use crate::nosman::workspace::Workspace;

pub struct SdkInfoCommand {
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SdkInfo {
    pub version: String,
    pub process_sdk_version: String,
    pub plugin_sdk_version: String,
    pub subsystem_sdk_version: String,
    pub path: String,
}

pub fn get_engine_sdk_infos(workspace: &Workspace) -> Result<Vec<SdkInfo>, CommandError> {
    let workspace_dir = &workspace.root;
    let engines_dir = get_default_engines_dir(workspace_dir);
    if !engines_dir.exists() {
        return Err(CommandError::InvalidArgument { message: "No Engine directory found in workspace".to_string() });
    }

    let mut result = Vec::new();
    // For each folder in engines_dir, check if it has SDK/version.json
    for entry in std::fs::read_dir(engines_dir)? {
        let entry = entry?;
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
        let info_str = std::fs::read_to_string(&info_file).unwrap_or_else(|e| panic!("Failed to read SDK info file {:?}: {}", info_file, e));
        let info_json: serde_json::Value = serde_json::from_str(&info_str).unwrap_or_else(|e| panic!("Failed to parse SDK info file {:?}: {}", info_file, e));
        let version = common::get_string(&info_json, "version", &info_file);
        let process_sdk_version = common::get_string(&info_json, "process_sdk_version", &info_file);
        let plugin_sdk_version = common::get_string(&info_json, "plugin_sdk_version", &info_file);
		// Try to get subsystem_sdk_version, if not found, return plugin_sdk_version since they are merged
        let subsystem_sdk_version = info_json.get("subsystem_sdk_version")
			.and_then(|v| v.as_str())
			.unwrap_or_else(|| plugin_sdk_version);
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
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct SdkInfoOutput {
    version: String,
    path: String,
}

impl SdkInfoCommand {
    pub fn run_get_sdk_info(&self, workspace: &Workspace, requested_version: &str, sdk_type: &str) -> CommandResult {
        // Search ./Engine directory under workspace dir and find the version.json with bin/ include/ folders in it
        let engines = get_engine_sdk_infos(workspace)?;

		let mut selected_versions = engines.iter().map(|x| {
			match sdk_type {
				"engine" => SdkInfoOutput { version: x.version.to_string(), path: x.path.to_string() },
				"plugin" => SdkInfoOutput { version: x.plugin_sdk_version.to_string(), path: x.path.to_string() },
				"subsystem" => SdkInfoOutput { version: x.subsystem_sdk_version.to_string(), path: x.path.to_string() },
				"process" => SdkInfoOutput { version: x.process_sdk_version.to_string(), path: x.path.to_string() },
				_ => return SdkInfoOutput{version: "".to_string(), path: "".to_string()},
			}
		}).collect::<Vec<SdkInfoOutput>>();


        // Sort the engines by version, latest first
        selected_versions.sort_by(|a, b| {
            let a_sem_ver = SemVer::parse_from_str(&a.version).unwrap_or_else(|| panic!("Failed to parse SDK version {}: {}", a.version, a.path));
            let b_sem_ver = SemVer::parse_from_str(&b.version).unwrap_or_else(|| panic!("Failed to parse SDK version {}: {}", b.version, b.path));
            b_sem_ver.cmp(&a_sem_ver)
        });

        let requested_sem_ver = match SemVer::parse_from_str(requested_version) {
            Some(semver) => semver,
            None => return Err(CommandError::InvalidArgument { message: format!("Invalid version: {}", requested_version) }),
        };

        let mut found_sdk_info: Option<SdkInfoOutput> = None;
        // Determine the correct version key based on sdk_type
        for sdk_info in selected_versions {
            let sdk_sem_ver = SemVer::parse_from_str(&sdk_info.version).unwrap_or_else(|| panic!("Failed to parse SDK version {}: {}", sdk_info.version, sdk_info.path));
            if sdk_sem_ver.satisfies_requested_version(&requested_sem_ver) {
                found_sdk_info = Some(sdk_info);
                break;
            }
        }
        if let Some(info) = found_sdk_info {
            println!("{}", serde_json::to_string_pretty(&info).unwrap());
            return Ok(());
        }

        Err(CommandError::InvalidArgument { message: format!("No SDK found for version {}", requested_version) })
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("sdk-info")
        .about("Returns information about an installed Nodos SDK under workspace.\n\
    If no such version is found, it will return an error.")
        .arg(Arg::new("version").required(true))
        .arg(Arg::new("sdk-type").required(false)
            .help("Type of the SDK to get information about.")
            .default_value("engine")
            .value_parser(clap::builder::PossibleValuesParser::new(["engine", "plugin", "subsystem", "process"])))
}

impl Command for SdkInfoCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("sdk-info")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
		let version = args.get_one::<String>("version").unwrap();
		let sdk_type_opt = args.get_one::<String>("sdk-type").map(|s| s.as_str());
		let sdk_type = sdk_type_opt.unwrap_or("engine");
        self.run_get_sdk_info(workspace, version, sdk_type)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
