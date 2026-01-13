use std::{fmt, fs, ptr};
use std::ffi::CString;
use std::fmt::Display;
use std::os::raw::c_int;
use std::path::PathBuf;
use std::time::Duration;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use crate::nosman::common::{get_progress_bar, NODOS_1_4};
use crate::nosman::{constants, extensions};
use crate::nosman::command::CommandError::Runtime;
use crate::nosman::command::CommandResult;
use crate::nosman::extensions::{CNosArg, CNosCommand, CNosRunCommandParams, NosCommand, NosCommandDesc};
use crate::nosman::index::{PackageType, PluginType, SemVer};
use crate::nosman::module::load_module_from_manifest;
use crate::nosman::path::{get_package_manifest_file, get_rel_path_based_on};
use crate::nosman::workspace::Workspace;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct PackageIdentifier {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct PackageInfo {
    pub id: PackageIdentifier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<PackageIdentifier>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct LocalPackageEntry {
    #[serde(alias = "module_type")]
    pub package_type: PackageType,
    #[serde(alias = "config_path")]
    pub manifest_path: PathBuf,
    pub info: PackageInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_include_folder: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub type_schema_files: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub commands: Vec<NosCommandDesc>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sdk_version: Option<String>
}

impl Display for LocalPackageEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.info.id, self.manifest_path.display())
    }
}

impl LocalPackageEntry {
    pub fn new(workspace: &Workspace, path: PathBuf, package_type: PackageType, register_commands: bool) -> Result<LocalPackageEntry, String> {
        let mut package = LocalPackageEntry {
            package_type,
            info: PackageInfo {
                id: PackageIdentifier {
                    name: String::new(),
                    version: String::new(),
                },
                display_name: None,
                description: None,
                dependencies: None,
                category: None,
                tags: None,
            },
            manifest_path: path.clone(),
            public_include_folder: None,
            type_schema_files: Vec::new(),
            commands: Vec::new(),
            sdk_version: None
        };

        let abs_path = workspace.root.join(&path);
        let file = match fs::File::open(&abs_path) {
            Ok(file) => file,
            Err(ref e) => {
                return Err(format!("Error reading file {}: {}", abs_path.display(), e).as_str().red().to_string());
            }
        };

        let res: Result<serde_json::Value, serde_json::Error> = serde_json::from_reader(file);
        if let Err(ref e) = res {
            return Err(format!("Error parsing file {}: {}", path.display(), e).as_str().red().to_string());
        }
        let manifest = res.unwrap();
        package.info = serde_json::from_value(manifest["info"].clone()).unwrap_or_else(|e| panic!("Failed to parse package info from {:?}: {}", path, e));

        // Check custom_types field
        if let Some(custom_types) = manifest["custom_types"].as_array() {
            for custom_type_file in custom_types {
                let type_file = abs_path.parent().unwrap().join(custom_type_file.as_str().unwrap());
                if !type_file.exists() {
                    return Err(format!("Package {} ({}) references a non-existent data schema file: {}", package.info.id.name, path.display(), type_file.display()).as_str().red().to_string());
                }
                package.type_schema_files.push(get_rel_path_based_on(&type_file.canonicalize().unwrap(), &workspace.root));
            }
        }

        // Check include folder
        if let Some(parent) = abs_path.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                let include_path: Option<PathBuf> = entries
                    .filter_map(Result::ok)
                    .find_map(|entry| {
                        entry
                            .file_name()
                            .to_str()
                            .filter(|name| name.eq_ignore_ascii_case("include"))
                            .map(|_| entry.path())
                    });

                if let Some(path) = include_path {
                    package.public_include_folder = Some(get_rel_path_based_on(&path.canonicalize().unwrap(), &workspace.root));
                }
            }
        }
        if register_commands {
            package.register_commands(&workspace);
        }

        package.sdk_version = manifest
            .get("sdk_version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(package)
    }
    pub fn get_package_root(&self) -> PathBuf {
        self.manifest_path.parent().unwrap().to_path_buf()
    }
    pub fn read_manifest(&self) -> serde_json::Value {
        // Read module manifest file as JSON, and read node definition files
        let manifest_file = fs::File::open(&self.manifest_path)
            .unwrap_or_else(|e| panic!("Failed to open package manifest file {:?}: {}", self.manifest_path, e));
        let manifest_json: serde_json::Value = serde_json::from_reader(manifest_file)
            .unwrap_or_else(|e| panic!("Failed to parse package manifest file {:?}: {}", self.manifest_path, e));
        manifest_json
    }
    pub fn register_commands(&mut self, workspace: &Workspace) {
        let res = load_module_from_manifest(&self, workspace);
        let lib = match res {
            Ok(lib) => lib,
            Err(_) => {
                return;
            }
        };
        if let Some(commands) = extensions::get_commands(lib) {
            for command in commands {
                self.commands.push(command);
            }
        }
    }
    pub fn get_abs_manifest_path(&self, workspace: &Workspace) -> PathBuf {
        workspace.root.join(&self.manifest_path)
    }
    pub fn run_command(&self, workspace: &Workspace, command_name: &str, params: NosCommand) -> CommandResult {
        let lib = load_module_from_manifest(&self, workspace)?;
        let fn_name = b"nosRunCommand\0";
        let res = unsafe { lib.get::<unsafe extern "C" fn(*const CNosRunCommandParams) -> c_int>(fn_name) };
        match res {
            Ok(fn_run_command) => {
                // Store the CStrings to keep them alive for the lifetime of the function call
                let command_name_cstr = CString::new(command_name).expect("CString::new failed for command_name");
                let workspace_dir = workspace.root.to_str().unwrap();
                let workspace_dir_cstr = CString::new(workspace_dir).expect("CString::new failed for workspace_dir");

                // Hold CString instances for the arguments
                let args_cstr_vec: Vec<_> = params.args.iter()
                    .map(|arg| {
                        let name_cstr = CString::new(arg.name.as_str()).expect("CString::new failed for arg name");
                        let value_cstr = CString::new(arg.value.as_str()).expect("CString::new failed for arg value");
                        (name_cstr, value_cstr)
                    })
                    .collect();

                // Create CNosArg array with pointers to CString data
                let mut args_cstr: Vec<_> = args_cstr_vec.iter()
                    .map(|(name_cstr, value_cstr)| CNosArg {
                        name: name_cstr.as_ptr(),
                        value: value_cstr.as_ptr(),
                    })
                    .collect();

                // Create the command struct
                let mut c_command = CNosCommand {
                    name: command_name_cstr.as_ptr(),
                    args_count: args_cstr.len(),
                    args: args_cstr.as_mut_ptr(),
                    sub_command: ptr::null_mut(), // TODO: Subcommands
                };

                // Prepare the parameters for fn_run_command
                let c_run_command_params = CNosRunCommandParams {
                    command: &mut c_command,
                    workspace_dir: workspace_dir_cstr.as_ptr(),
                };

                // Call the function and handle the result
                let res = unsafe { fn_run_command(&c_run_command_params) };
                if res == 0 {
                    Ok(())
                } else {
                    Err(Runtime { message: format!("Command {} returned with code {}", command_name, res) })
                }
            }
            Err(_) => Err(Runtime { message: format!("Failed to get function {}", std::str::from_utf8(fn_name).unwrap()) })
        }
    }
    pub fn needs_rescan(&self, workspace: &Workspace, compare_commands: bool) -> bool {
        if !workspace.root.join(&self.manifest_path).exists() {
            return true;
        }
        let res = LocalPackageEntry::new(workspace, self.manifest_path.clone(),
                                         self.package_type.clone(),
                                         /* we might consider not loading CLI extensions here and discarding it from comparison at return */
                                         compare_commands);
        if let Err(msg) = res {
            eprintln!("{}", msg);
            return true;
        }
        let package = res.unwrap();
        self.package_type != package.package_type
            || self.manifest_path != package.manifest_path
            || self.info != package.info
            || self.public_include_folder != package.public_include_folder
            || self.type_schema_files != package.type_schema_files
            || (compare_commands && self.commands != package.commands)
    }
}

impl fmt::Display for PackageIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}

pub fn get_package_manifests(folder: &PathBuf, silent: bool) -> Vec<(PackageType, PathBuf)> {
    let pb = get_progress_bar(silent);
    pb.enable_steady_tick(Duration::from_millis(100));

    pb.set_message(format!("Looking for Nodos packages in {}", folder.to_str().unwrap_or_else(|| panic!("Non-UTF-8 path: {}", folder.display()))).to_string());
    let res = get_package_manifest_file(&folder);
    if res.is_ok() {
        if let Some((ty, mpath)) = res.unwrap() {
            return vec![(ty, mpath)];
        }
    }

    let patterns = &[
        format!("*.{{{},{},{},{}}}",
                constants::PLUGIN_MANIFEST_FILE_EXT, constants::LEGACY_SUBSYSTEM_MANIFEST_FILE_EXT,
                constants::LEGACY_PLUGIN_MANIFEST_FILE_EXT, constants::GENERIC_PACKAGE_MANIFEST_FILE_EXT),
        "!**/.git/**".to_string()
    ];
    let walker = globwalk::GlobWalkerBuilder::from_patterns(folder, patterns)
        .file_type(globwalk::FileType::FILE)
        .build()
        .unwrap_or_else(|e| panic!("Failed to glob dirs: {:?}: {}", patterns, e));
    let mut package_manifest_files = vec![];
    for entry in walker {
        match entry {
            Ok(entry) => {
                let path = entry.path().to_path_buf();
                // If multiple manifest files are found in the same folder, we will skip this folder
                let parent = path.parent().unwrap_or_else(|| panic!("No parent folder found for path: {}", path.display())).to_path_buf();
                let res = get_package_manifest_file(&parent);
                if let Ok(res) = res {
                    if let Some((ty, mpath)) = res {
                        package_manifest_files.push((ty, mpath));
                    }
                }
            }
            Err(e) => {
                pb.println(format!("Error while walking: {}", e));
            }
        }
    }

    pb.finish_and_clear();
    package_manifest_files
}

pub fn get_plugin_manifest_file_ext(nodos_version: Option<&SemVer>, plugin_type: &PluginType) -> &'static str {
    let ext = if nodos_version.is_some() && *nodos_version.unwrap() >= NODOS_1_4 {
        constants::PLUGIN_MANIFEST_FILE_EXT
    } else if *plugin_type == PluginType::Default {
        constants::LEGACY_PLUGIN_MANIFEST_FILE_EXT
    } else {
        constants::LEGACY_SUBSYSTEM_MANIFEST_FILE_EXT
    };
    ext
}

pub fn get_package_info_from_manifest(manifest_path: &PathBuf) -> Result<PackageInfo, String> {
    let file = fs::File::open(manifest_path)
        .map_err(|e| format!("Failed to open package manifest file {:?}: {}", manifest_path, e))?;
    let manifest: serde_json::Value = serde_json::from_reader(file)
        .map_err(|e| format!("Failed to parse package manifest file {:?}: {}", manifest_path, e))?;
    Ok(serde_json::from_value(manifest["info"].clone()).unwrap_or_else(|e| panic!("Failed to parse package info from {:?}: {}", manifest_path, e)))
}