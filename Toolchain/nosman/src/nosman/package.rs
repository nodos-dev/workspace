use std::{fmt, fs, ptr};
use std::ffi::CString;
use std::fmt::Display;
use std::os::raw::c_int;
use std::path::PathBuf;
use std::time::Duration;
use colored::Colorize;
use inquire::Text;
use serde::{Deserialize, Serialize};
use crate::nosman::common::{get_nodos_version, get_progress_bar, NODOS_1_4};
use crate::nosman::{constants, extensions};
use crate::nosman::command::CommandError::Runtime;
use crate::nosman::command::CommandResult;
use crate::nosman::extensions::{CNosArg, CNosCommand, CNosRunCommandParams, NosCommand, NosCommandDesc};
use crate::nosman::index::{PackageType, PluginType, SemVer};
use crate::nosman::module::{load_module_from_manifest};
use crate::nosman::plugin::{NodeDefinition};
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
pub struct GenericPackageManifest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    pub info: PackageInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_extension_bin_path: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub type_schema_files: Vec<String>,
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
    pub commands: Vec<NosCommandDesc>
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
        package.info = serde_json::from_value(manifest["info"].clone()).unwrap_or_else(|e| panic!("Failed to parse module info from {:?}: {}", path, e));

        // Check custom_types field
        if let Some(custom_types) = manifest["custom_types"].as_array() {
            for custom_type_file in custom_types {
                let type_file = abs_path.parent().unwrap().join(custom_type_file.as_str().unwrap());
                if !type_file.exists() {
                    return Err(format!("Module {} ({}) references a non-existent data schema file: {}", package.info.id.name, path.display(), type_file.display()).as_str().red().to_string());
                }
                package.type_schema_files.push(get_rel_path_based_on(&type_file.canonicalize().unwrap(), &workspace.root));
            }
        }

        // Check include folder
        if abs_path.parent().unwrap().join("Include").exists() {
            package.public_include_folder = Some(get_rel_path_based_on(&abs_path.parent().unwrap().join("Include").canonicalize().unwrap(), &workspace.root));
        }
        if register_commands {
            package.register_commands(&workspace);
        }
        Ok(package)
    }
    pub fn get_package_root(&self) -> PathBuf {
        self.manifest_path.parent().unwrap().to_path_buf()
    }
    pub fn read_manifest(&self) -> serde_json::Value {
        // Read module manifest file as JSON, and read node definition files
        let manifest_file = fs::File::open(&self.manifest_path)
            .unwrap_or_else(|e| panic!("Failed to open module manifest file {:?}: {}", self.manifest_path, e));
        let manifest_json: serde_json::Value = serde_json::from_reader(manifest_file)
            .unwrap_or_else(|e| panic!("Failed to parse module manifest file {:?}: {}", self.manifest_path, e));
        manifest_json
    }
    pub fn get_node_definition(&self, class_name: &str, nodos_version: &Option<SemVer>) -> Option<NodeDefinition> {
        if self.package_type != PackageType::Plugin {
            return None;
        }
        // Read module manifest file as JSON, and read node definition files
        let manifest_json = self.read_manifest();
        let node_defs_rel_paths_opt = manifest_json["node_definitions"].as_array();
        let mut node_defs_rel_paths = vec![];
        if node_defs_rel_paths_opt.is_none() {
            if let Some(nodos_version) = nodos_version {
                if *nodos_version >= NODOS_1_4 {
                    // Find .nosnode files under Nodes/ folder
                    let nodes_dir = self.get_package_root().join("Nodes");
                    if !nodes_dir.exists() {
                        return None;
                    }
                    for entry in fs::read_dir(&nodes_dir).unwrap_or_else(|e| panic!("Failed to read Nodes directory {:?}: {}", nodes_dir, e)) {
                        let entry = entry.unwrap();
                        let path = entry.path();
                        if path.is_file() && path.extension().map_or(false, |ext| ext == constants::NODE_DEFINITION_FILE_EXT) {
                            let rel_path = get_rel_path_based_on(&path.canonicalize().unwrap(), &self.get_package_root());
                            let rel_path_str = rel_path.to_string_lossy().to_string();
                            node_defs_rel_paths.push(rel_path_str);
                        }
                    }
                }
            }
        } else {
            node_defs_rel_paths = node_defs_rel_paths_opt.unwrap()
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        for node_defs_rel_path in node_defs_rel_paths {
            let node_defs_path = self.get_package_root().join(node_defs_rel_path.as_str());
            let node_defs_file_content = fs::read_to_string(&node_defs_path);
            if let Err(e) = node_defs_file_content {
                eprintln!("{}", format!("Failed to read node definitions file ({}): {}", node_defs_path.display(), e).red());
                continue;
            }
            let node_defs_file_content = node_defs_file_content.unwrap();
            // Remove BOM
            let node_defs_file_content = node_defs_file_content.trim_start_matches('\u{FEFF}');
            let node_defs: serde_json::Value = serde_json::from_str(node_defs_file_content).unwrap_or_else(|e| panic!("Failed to parse node definitions file {:?}: {}", node_defs_path, e));
            let nodes_json_array = node_defs.get("nodes").unwrap_or_else(|| panic!("Missing 'nodes' field in node definitions file {:?}", node_defs_path))
                .as_array().unwrap_or_else(|| panic!("'nodes' field is not an array: {}", node_defs_path.display()));
            for (index, node_json) in nodes_json_array.iter().enumerate() {
                let mut curr_class_name = node_json["class_name"].as_str().unwrap_or_else(|| panic!("Missing 'class_name' field in node definition in {:?}", node_defs_path)).to_string();
                // If class name is not prefixed with module name, prefix it
                if !curr_class_name.starts_with(self.info.id.name.as_str()) {
                    curr_class_name = format!("{}.{}", self.info.id.name, curr_class_name);
                }
                if curr_class_name == *class_name {
                    return Some (NodeDefinition {
                        class_name: curr_class_name.to_string(),
                        defined_in: node_defs_path.clone(),
                        index,
                        json: node_defs.clone(),
                        owner: self.clone(),
                    });
                }
            }
        }
        None
    }
    pub fn remove_node_definition(&self, node_class_name: &String, nodos_version: Option<SemVer>) -> Result<(), String> {
        let node_def = self.get_node_definition(node_class_name.as_str(), &nodos_version);
        if node_def.is_none() {
            return Err(format!("Node class {} not found in plugin {}", node_class_name, self));
        }
        let mut node_def = node_def.unwrap();
        let node_defs_list = node_def.json["nodes"].as_array().cloned();
        if node_defs_list.is_none() {
            return Err(format!("Node definitions file {} does not contain a 'nodes' array", node_def.defined_in.display()));
        }
        let mut node_defs_list = node_defs_list.unwrap();
        let update_manifest = node_defs_list.len() == 1;
        node_defs_list.remove(node_def.index);
        node_def.json["nodes"] = serde_json::Value::Array(node_defs_list.clone());
        let node_defs_list = node_defs_list;
        // Write back to file
        let node_defs_file_content = serde_json::to_string_pretty(&node_def.json).unwrap_or_else(|e| panic!("Failed to serialize node definitions at {}: {}", node_def.defined_in.display(), e));
        fs::write(&node_def.defined_in, node_defs_file_content).unwrap_or_else(|e| panic!("Failed to write node definitions file {}: {}", node_def.defined_in.display(), e));

        let node_def_path = dunce::canonicalize(&node_def.defined_in).unwrap_or_else(|e| panic!("Failed to canonicalize node definition path: {} {}", node_def.defined_in.display(), e));
        if update_manifest {
            let mut manifest_json = self.read_manifest();
            if !manifest_json["node_definitions"].is_null() {
                let node_defs_rel_paths = manifest_json["node_definitions"].as_array_mut().unwrap();
                // Remove the node definition file from the manifest
                node_defs_rel_paths.retain(|path| {
                    let path_obj = self.get_package_root().join(PathBuf::from(path.as_str()
                        .unwrap_or_else(|| panic!("Failed to convert path to string: {}", path))));
                    let path_obj = dunce::canonicalize(&path_obj).unwrap_or_else(|e| panic!("Failed to canonicalize path: {} {}", path_obj.display(), e));
                    path_obj != node_def_path
                });
                // Write back to manifest
                let manifest_str = serde_json::to_string_pretty(&manifest_json).unwrap_or_else(|e| panic!("Failed to serialize module manifest: {}", e));
                fs::write(&self.manifest_path, manifest_str).unwrap_or_else(|e| panic!("Failed to write module manifest file {}: {}", self.manifest_path.display(), e));
            }
        }
        // Now, if the node definition file is empty, remove it
        if node_defs_list.is_empty() {
            fs::remove_file(&node_def.defined_in)
                .unwrap_or_else(|e| panic!("Failed to remove node definitions file {}: {}", node_def.defined_in.display(), e));
        }
        Ok(())
    }
    pub fn add_node_definition(&self, workspace: &Workspace, node_class_name: &String, display_name: Option<String>, description: Option<String>, category: Option<String>,
                               hide_in_context_menu: bool, nodos_version: Option<SemVer>) -> Result<(), String> {
        println!("{}", format!("Adding a node '{}' to plugin: {}", node_class_name, self).green());
        let node_class_name = if node_class_name.starts_with(self.info.id.name.as_str()) {
            node_class_name.clone()
        }
        else {
            format!("{}.{}", self.info.id.name, node_class_name)
        };
        let node_def = self.get_node_definition(&node_class_name, &nodos_version);
        if node_def.is_some() {
            return Err(format!("Node class {} already exists in plugin {}", node_class_name, self));
        }
        let display_name = display_name.unwrap_or_else(|| {
            let default_display_name = node_class_name.strip_prefix(format!("{}.", &self.info.id.name).as_str())
                .unwrap_or(&node_class_name);
            Text::new("Display name:")
                .with_default(default_display_name)
                .prompt()
                .unwrap_or_else(|e| panic!("Failed to get display name: {}", e))
        });
        let description = description.unwrap_or_else(|| {
            Text::new("Description:")
                .with_default(format!("{} node", display_name).as_str())
                .prompt()
                .unwrap_or_else(|e| panic!("Failed to get description: {}", e))
        });
        let category = category.unwrap_or_else(|| {
            Text::new("Category:")
                .with_default("Custom")
                .prompt()
                .unwrap_or_else(|e| panic!("Failed to get category: {}", e))
        });

        let selected_version = get_nodos_version(workspace, &nodos_version)?;
        let mut manifest_json = self.read_manifest();
        if manifest_json["node_definitions"].is_null() {
            manifest_json["node_definitions"] = serde_json::json!([]);
        }
        let node_defs_rel_paths = manifest_json["node_definitions"].as_array_mut().unwrap();
        let update_manifest = selected_version < NODOS_1_4 || node_defs_rel_paths.len() > 0;
        let node_def_file_ext = if selected_version < NODOS_1_4 {
            constants::LEGACY_NODE_DEFINITION_FILE_EXT
        } else {
            constants::NODE_DEFINITION_FILE_EXT
        };


        let out_node_defs_file = format!("Nodes/{}", node_class_name.strip_prefix(format!("{}.", &self.info.id.name).as_str()).unwrap_or_else(|| &node_class_name));
        println!("Node definition file: {}", out_node_defs_file);
        let node_def_path = PathBuf::from(&out_node_defs_file).with_extension(node_def_file_ext).to_path_buf();
        node_defs_rel_paths.push(serde_json::Value::String(node_def_path.to_str()
            .unwrap_or_else(|| panic!("Failed to convert path to string: {}", node_def_path.display())).to_string()));
        let out_node_defs_path = self.get_package_root().join(&out_node_defs_file);
        // Write node definitions file
        let node_defs = serde_json::json!({
            "nodes": [
                {
                    "class_name": node_class_name,
                    "menu_info": {
                        "category": category,
                        "display_name": display_name,
                        "hide_in_context_menu": hide_in_context_menu,
                    },
                    "node": {
                        "contents_type": "Job",
                        "display_name": display_name,
                        "description": description,
                        "pins": []
                    }
                }
            ]
        });
        let node_defs_str = serde_json::to_string_pretty(&node_defs).unwrap_or_else(|e| panic!("Failed to serialize node definitions: {}", e));
        fs::create_dir_all(out_node_defs_path.parent().unwrap())
            .unwrap_or_else(|e| panic!("Failed to create node definitions file parent directory {}: {}", out_node_defs_path.display(), e));
        // If no extension, add it
        let out_node_defs_path = if out_node_defs_path.extension().is_none() {
            out_node_defs_path.with_extension(node_def_file_ext)
        } else {
            out_node_defs_path
        };
        fs::write(&out_node_defs_path, node_defs_str).unwrap_or_else(|e| panic!("Failed to write node definitions file {}: {}", out_node_defs_path.display(), e));
        if update_manifest {
            let manifest_str = serde_json::to_string_pretty(&manifest_json).unwrap_or_else(|e| panic!("Failed to serialize module manifest: {}", e));
            fs::write(&self.manifest_path, manifest_str).unwrap_or_else(|e| panic!("Failed to write module manifest file {}: {}", self.manifest_path.display(), e));
        }
        Ok(())
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
    pub fn needs_rescan(&self, workspace: &Workspace) -> bool {
        if !workspace.root.join(&self.manifest_path).exists() {
            return true;
        }
        let res = LocalPackageEntry::new(workspace, self.manifest_path.clone(),
                                        self.package_type.clone(),
                                       /* we might consider not loading CLI extensions here and discarding it from comparison at return */
                                       true);
        if let Err(msg) = res {
            eprintln!("{}", msg);
            return true;
        }
        let installed_module = res.unwrap();
        &installed_module != self
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

    pb.set_message(format!("Looking for Nodos modules in {}", folder.to_str().unwrap_or_else(|| panic!("Non-UTF-8 path: {}", folder.display()))).to_string());
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