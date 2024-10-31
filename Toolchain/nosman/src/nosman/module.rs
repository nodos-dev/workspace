use serde::{Deserialize, Serialize};
use std::{fmt, fs, ptr};
use std::ffi::{CString, OsString};
use std::fmt::Display;
use std::os::raw::c_int;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::Duration;
use colored::Colorize;
use indicatif::{ProgressBar};
use inquire::Text;
use libloading::Library;
use crate::nosman::command::{CommandError, CommandResult};
use crate::nosman::command::CommandError::{InvalidArgumentError, RuntimeError};
use crate::nosman::{constants, extensions};
use crate::nosman::extensions::{CNosArg, CNosCommand, CNosRunCommandParams, NosCommand, NosCommandDesc};
use crate::nosman::index::{ModuleType};
use crate::nosman::path::{get_plugin_manifest_file, get_subsystem_manifest_file};
use crate::nosman::platform::get_host_platform;
use crate::nosman::workspace::Workspace;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct PackageIdentifier {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct ModuleInfo {
    pub id: PackageIdentifier,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub dependencies: Option<Vec<PackageIdentifier>>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Hash, Clone, Eq, PartialEq)]
pub struct InstalledModule {
    pub info: ModuleInfo,
    #[serde(alias = "config_path")]
    pub manifest_path: PathBuf,
    pub public_include_folder: Option<PathBuf>,
    pub type_schema_files: Vec<PathBuf>,
    pub module_type: ModuleType,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<NosCommandDesc>
}

impl Display for InstalledModule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.info.id, self.manifest_path.display())
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct NodeDefinition {
    pub class_name: String,
    pub defined_in: PathBuf,
    pub index: usize,
    pub node_defs_json: serde_json::Value,
    pub owner: InstalledModule,
}

impl Display for NodeDefinition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.owner.info.id, self.defined_in.display())
    }
}

impl InstalledModule {
    pub fn new(path: PathBuf) -> InstalledModule {
        InstalledModule {
            info: ModuleInfo {
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
            manifest_path: path,
            public_include_folder: None,
            type_schema_files: Vec::new(),
            module_type: ModuleType::Plugin,
            commands: Vec::new(),
        }
    }
    pub fn get_module_dir(&self) -> PathBuf {
        self.manifest_path.parent().unwrap().to_path_buf()
    }
    pub fn read_manifest(&self) -> Result<serde_json::Value, String> {
        // Read module manifest file as JSON, and read node definition files
        let manifest_file = fs::File::open(&self.manifest_path);
        if let Err(e) = manifest_file {
            return Err(format!("Failed to open module manifest file ({}): {}", self.manifest_path.display(), e));
        }
        let manifest_file = manifest_file.unwrap();
        let manifest_json: serde_json::Value = serde_json::from_reader(manifest_file).expect("Failed to parse manifest file");
        Ok(manifest_json)
    }
    pub fn get_node_definition(&self, class_name: &str) -> Option<NodeDefinition> {
        if self.module_type != ModuleType::Plugin {
            return None;
        }
        // Read module manifest file as JSON, and read node definition files
        let manifest_json = self.read_manifest().expect(format!("Failed to read module manifest file ({})", self.manifest_path.display()).as_str());
        let node_defs_rel_paths = manifest_json["node_definitions"].as_array();
        if node_defs_rel_paths.is_none() {
            return None;
        }
        for node_defs_rel_path in node_defs_rel_paths? {
            let node_defs_path = self.get_module_dir().join(node_defs_rel_path.as_str()?);
            let node_defs_file_content = fs::read_to_string(&node_defs_path);
            if let Err(e) = node_defs_file_content {
                eprintln!("{}", format!("Failed to read node definitions file ({}): {}", node_defs_path.display(), e).red());
                continue;
            }
            let node_defs_file_content = node_defs_file_content.unwrap();
            // Remove BOM
            let node_defs_file_content = node_defs_file_content.trim_start_matches('\u{FEFF}');
            let node_defs: serde_json::Value = serde_json::from_str(&node_defs_file_content).expect(format!("Failed to parse node definitions file: {}", node_defs_path.display()).as_str());
            let nodes_json_array = node_defs.get("nodes").expect("Missing 'nodes' field in node definitions file").as_array().expect("'nodes' field is not an array");
            let mut index = 0;
            for node_json in nodes_json_array {
                let mut curr_class_name = node_json["class_name"].as_str().expect(format!("Missing 'class_name' field in node definition in {}", node_defs_path.display()).as_str()).to_string();
                // If class name is not prefixed with module name, prefix it
                if !curr_class_name.starts_with(self.info.id.name.as_str()) {
                    curr_class_name = format!("{}.{}", self.info.id.name, curr_class_name);
                }
                if curr_class_name == *class_name {
                    return Some (NodeDefinition {
                        class_name: curr_class_name.to_string(),
                        defined_in: node_defs_path.clone(),
                        index,
                        node_defs_json: node_defs.clone(),
                        owner: self.clone(),
                    });
                }
                index += 1;
            }
        }
        None
    }
    pub fn remove_node_definition(&self, node_class_name: &String) -> bool {
        let node_def = self.get_node_definition(node_class_name.as_str());
        if node_def.is_none() {
            return false;
        }
        let mut node_def = node_def.unwrap();
        // Remove from defined_in
        node_def.node_defs_json["nodes"].as_array_mut().unwrap().remove(node_def.index);
        // Write back to file
        let node_defs_file_content = serde_json::to_string_pretty(&node_def.node_defs_json).expect("Failed to serialize node definitions");
        fs::write(&node_def.defined_in, node_defs_file_content).expect("Failed to write node definitions file");

        // Read manifest and remove from associated_nodes
        let mut manifest_json = self.read_manifest().expect("Failed to read module manifest file");
        let associated_nodes = manifest_json["associated_nodes"].as_array_mut().expect("Missing 'associated_nodes' field in module manifest file");
        associated_nodes.retain(|node| {
            let class_name = node["class_name"].as_str().expect("Missing 'class_name' field in associated node");
            class_name != node_class_name
        });
        // Update manifest file
        let manifest_str = serde_json::to_string_pretty(&manifest_json).expect("Failed to serialize manifest");
        fs::write(&self.manifest_path, manifest_str).expect("Failed to write manifest file");
        true
    }
    pub fn add_node_definition(&self, node_class_name: &String, display_name: Option<String>, description: Option<String>, category: Option<String>, hide_in_context_menu: bool) -> Result<(), String> {
        println!("{}", format!("Adding a node '{}' to plugin: {}", node_class_name, self).green());
        let node_class_name = if node_class_name.starts_with(self.info.id.name.as_str()) {
            node_class_name.clone()
        }
        else {
            format!("{}.{}", self.info.id.name, node_class_name)
        };
        let node_def = self.get_node_definition(&node_class_name);
        if node_def.is_some() {
            return Err(format!("Node class {} already exists in plugin {}", node_class_name, self));
        }
        let display_name = display_name.unwrap_or(Text::new("Display name:").prompt().unwrap());
        let description = description.unwrap_or(Text::new("Description:").prompt().unwrap());
        let category = category.unwrap_or(Text::new("Category:").prompt().unwrap());

        let mut manifest_json = self.read_manifest().expect("Failed to read module manifest file");
        let node_defs_rel_paths = manifest_json["node_definitions"].as_array_mut().expect("Missing 'node_definitions' field in module manifest file");
        let out_node_defs_file = Text::new("Node definitions file:")
            .with_default(format!("Config/{}", node_class_name.strip_prefix(format!("{}.", &self.info.id.name).as_str()).unwrap()).as_str()).prompt().expect("Failed to get node definitions file");
        node_defs_rel_paths.push(serde_json::Value::String(PathBuf::from(&out_node_defs_file).with_extension(constants::NODE_DEF_FILE_EXT).to_path_buf().to_str().expect("Failed to convert path to string").to_string()));
        let out_node_defs_path = self.get_module_dir().join(&out_node_defs_file);
        // Write node definitions file
        let node_defs = serde_json::json!({
            "nodes": [
                {
                    "class_name": node_class_name,
                    "contents_type": "Job",
                    "display_name": display_name,
                    "description": description,
                    "pins": []
                }
            ]
        });
        let node_defs_str = serde_json::to_string_pretty(&node_defs).expect("Failed to serialize node definitions");
        fs::create_dir_all(out_node_defs_path.parent().expect("No parent found")).expect("Failed to create node definitions file parent directory");
        // If no .nosdef extension, add it
        let out_node_defs_path = if out_node_defs_path.extension().is_none() {
            out_node_defs_path.with_extension(constants::NODE_DEF_FILE_EXT)
        } else {
            out_node_defs_path
        };
        fs::write(&out_node_defs_path, node_defs_str).expect("Failed to write node definitions file");
        // Write to associated_nodes in manifest
        let associated_nodes = manifest_json["associated_nodes"].as_array_mut().expect("Missing 'associated_nodes' field in module manifest file");
        associated_nodes.push(serde_json::json!({
            "class_name": node_class_name,
            "display_name": display_name,
            "category": category,
            "hide_in_context_menu": hide_in_context_menu,
        }));
        // Update manifest file
        let manifest_str = serde_json::to_string_pretty(&manifest_json).expect("Failed to serialize manifest");
        fs::write(&self.manifest_path, manifest_str).expect("Failed to write manifest file");
        Ok(())
    }
    pub fn register_commands(&mut self, workspace: &Workspace) {
        let res = load_installed_module(&self, workspace);
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
    pub fn run_command(&self, command_name: &str, params: NosCommand) -> CommandResult {
        let lib = load_installed_module(&self, Workspace::get().expect("Failed to get workspace"))?;
        let fn_name = b"nosRunCommand\0";
        let res = unsafe { lib.get::<unsafe extern "C" fn(*const CNosRunCommandParams) -> c_int>(fn_name) };
        match res {
            Ok(fn_run_command) => {
                // Store the CStrings to keep them alive for the lifetime of the function call
                let command_name_cstr = CString::new(command_name).expect("CString::new failed for command_name");
                let workspace_dir = Workspace::get()?.root.to_str().unwrap();
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
                    Ok(true)
                } else {
                    Err(RuntimeError { message: format!("Command {} returned with code {}", command_name, res) })
                }
            }
            Err(_) => Err(RuntimeError { message: format!("Failed to get function {}", std::str::from_utf8(fn_name).unwrap()) })
        }
    }
}

impl fmt::Display for PackageIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}

pub fn get_module_manifest_file_in_folder(folder: &PathBuf) -> Result<Option<(ModuleType, PathBuf)>, String> {
    let res = get_plugin_manifest_file(folder);
    if res.is_err() {
        return Err(res.err().unwrap());
    }
    let plugin_manifest_file = res?;
    let res = get_subsystem_manifest_file(folder);
    let subsystem_manifest_file = res?;
    if plugin_manifest_file.is_some() && subsystem_manifest_file.is_some() {
        return Err(format!("Multiple module manifest files found in {}", folder.display()));
    }
    if plugin_manifest_file.is_none() && subsystem_manifest_file.is_none() {
        return Ok(None);
    }
    if plugin_manifest_file.is_some() {
        return Ok(Some((ModuleType::Plugin, plugin_manifest_file.unwrap())));
    }
    Ok(Some((ModuleType::Subsystem, subsystem_manifest_file.unwrap())))
}

pub fn get_module_manifests(folder: &PathBuf) -> Vec<(ModuleType, PathBuf)> {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(100));

    pb.set_message(format!("Looking for Nodos modules in {}", folder.to_str().expect("Non-UTF-8 path")).to_string());
    let res = get_module_manifest_file_in_folder(&folder);
    if res.is_ok() {
        if let Some((ty, mpath)) = res.unwrap() {
            return vec![(ty, mpath)];
        }
    }

    let patterns = &[format!("*.{{{},{}}}", constants::SUBSYSTEM_MANIFEST_FILE_EXT, constants::PLUGIN_MANIFEST_FILE_EXT)];
    let walker = globwalk::GlobWalkerBuilder::from_patterns(folder, patterns)
        .file_type(globwalk::FileType::FILE)
        .build()
        .expect(format!("Failed to glob dirs: {:?}", patterns).as_str());
    let mut module_manifest_files = vec![];
    for entry in walker {
        match entry {
            Ok(entry) => {
                let path = entry.path().to_path_buf();
                // If multiple manifest files are found in the same folder, we will skip this folder
                let parent = path.parent().expect("No parent found").to_path_buf();
                let res = get_module_manifest_file_in_folder(&parent);
                if let Ok(res) = res {
                    if let Some((ty, mpath)) = res {
                        module_manifest_files.push((ty, mpath));
                    }
                }
            }
            Err(e) => {
                pb.println(format!("Error while walking: {}", e).to_string());
            }
        }
    }

    pb.finish_and_clear();
    module_manifest_files
}

pub fn load_module_with_search_paths(verbose: bool, binary_path: &OsString, additional_search_paths: Vec<PathBuf>) -> Result<Library, CommandError> {
    if verbose {
        println!("Loading dynamic library: {}", binary_path.to_str().unwrap());
    }
    #[cfg(unix)]
    {
        // Store the original environment variable values
        #[cfg(target_os = "linux")]
        let original_var = env::var_os("LD_LIBRARY_PATH");

        #[cfg(target_os = "macos")]
        let original_var = env::var_os("DYLD_LIBRARY_PATH");


        {
            for lib_dir in additional_search_paths {
                // Add this directory to the appropriate environment variable
                #[cfg(target_os = "linux")]
                {
                    let paths = env::var_os("LD_LIBRARY_PATH").unwrap_or_else(|| "".into());
                    let mut lib_dir = lib_dir.clone();
                    lib_dir.push(":");
                    lib_dir.push(paths);
                    env::set_var("LD_LIBRARY_PATH", lib_dir);
                }

                #[cfg(target_os = "macos")]
                {
                    let paths = env::var_os("DYLD_LIBRARY_PATH").unwrap_or_else(|| "".into());
                    let mut lib_dir = lib_dir.clone();
                    lib_dir.push(":");
                    lib_dir.push(paths);
                    env::set_var("DYLD_LIBRARY_PATH", lib_dir);
                }
            }
        }



        let res;
        // Now load the library
        unsafe {
            res = Library::new(&binary_path)
        }

        {
            // Restore the original environment variable values
            #[cfg(target_os = "linux")]
            if let Some(original) = original_var {
                env::set_var("LD_LIBRARY_PATH", original);
            } else {
                env::remove_var("LD_LIBRARY_PATH");
            }

            #[cfg(target_os = "macos")]
            if let Some(original) = original_var {
                env::set_var("DYLD_LIBRARY_PATH", original);
            } else {
                env::remove_var("DYLD_LIBRARY_PATH");
            }
        }

        if res.is_err() {
            return Err(RuntimeError { message: format!("Failed to load dynamic library: {}", res.err().unwrap()) });
        }
        Ok(res.unwrap())
    }

    #[cfg(target_os = "windows")]
    unsafe {
        // Set default DLL directories
        use winapi::um::libloaderapi::{SetDefaultDllDirectories, AddDllDirectory, RemoveDllDirectory};
        use winapi::um::libloaderapi::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
        if 0 == SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) {
            // Get last error
            let err = std::io::Error::last_os_error();
            return Err(RuntimeError { message: format!("Failed to set default DLL directories: {}", err) });
        }
        let mut dll_cookies = vec![];
        for lib_dir in additional_search_paths {
            if !lib_dir.exists() {
                println!("{}", format!("Warning: DLL search path {} does not exist", lib_dir.display()).yellow().to_string());
                continue;
            }
            let lib_dir_canonical = dunce::canonicalize(&lib_dir).expect(format!("Failed to canonicalize path: {}", lib_dir.display()).as_str());
            if verbose {
                println!("\tAdding DLL search path: {}", lib_dir_canonical.display());
            }
            let wdir: Vec<u16> = lib_dir_canonical.as_os_str().encode_wide().chain(Some(0)).collect();
            let cookie = AddDllDirectory(wdir.as_ptr());
            if cookie.is_null() {
                let err = std::io::Error::last_os_error();
                return Err(RuntimeError { message: format!("Failed to add DLL search path {}: {}", lib_dir_canonical.display(), err) });
            }
            dll_cookies.push(cookie);
        }
        let res = Library::new(&binary_path);
        for cookie in dll_cookies {
            RemoveDllDirectory(cookie);
        }
        if res.is_err() {
            return Err(RuntimeError { message: format!("Failed to load dynamic library: {}", res.err().unwrap()) });
        }
        Ok(res.unwrap())
    }
}

pub fn load_installed_module(module: &InstalledModule, workspace: &Workspace) -> Result<Library, CommandError> {
    let manifest_file_contents = fs::read_to_string(&module.get_abs_manifest_path(workspace)).expect("Failed to read module manifest file");
    let manifest: serde_json::Value = serde_json::from_str(&manifest_file_contents).expect("Failed to parse module manifest file");
    load_module(false, manifest, module.get_abs_manifest_path(workspace).parent().unwrap().to_path_buf(), workspace)
}

pub fn load_module(verbose: bool, manifest: serde_json::Value, manifest_file_parent: PathBuf, workspace: &Workspace) -> Result<Library, CommandError> {
    let binary_path = manifest["binary_path"].as_str();
    if binary_path.is_none() {
        return Err (InvalidArgumentError {message: "Module manifest does not specify a binary path".to_string() })
    }
    let module_dir = manifest_file_parent;
    let binary_path = module_dir.join(binary_path.unwrap());
    let host_platform = get_host_platform();
    let binary_path = binary_path.with_extension(
        if host_platform.os == "windows" { "dll" }
        else if host_platform.os == "macos" { "dylib" }
        else { "so" }
    ).into_os_string();
    let mut additional_search_paths: Vec<PathBuf> = Vec::new();
    for path_str in manifest["additional_search_paths"].as_array().unwrap_or(&vec![]).iter() {
        let path = module_dir.join(path_str.as_str().unwrap());
        additional_search_paths.push(path);
    }
    // Add search paths of dependencies
    for dep in manifest["info"]["dependencies"].as_array().unwrap_or(&vec![]) {
        let dep_name = dep["name"].as_str().unwrap();
        let dep_version = dep["version"].as_str().unwrap();
        let dep_res = workspace.get_latest_installed_module_for_version(dep_name, dep_version);
        if let Ok(installed_module) = dep_res {
            let dep_manifest_file_path = workspace.root.join(&installed_module.manifest_path);
            let dep_manifest_file_contents = std::fs::read_to_string(&dep_manifest_file_path).expect("Failed to read dependency manifest file");
            let dep_manifest: serde_json::Value = serde_json::from_str(&dep_manifest_file_contents).expect("Failed to parse dependency manifest file");
            for path_str in dep_manifest["additional_search_paths"].as_array().unwrap_or(&vec![]) {
                let module_dir = dep_manifest_file_path.parent().unwrap();
                let path = module_dir.join(path_str.as_str().unwrap());
                additional_search_paths.push(path);
            }
        }
    }
    // Load the dynamic library
    let lib = load_module_with_search_paths(verbose, &binary_path, additional_search_paths);
    if lib.is_err() {
        return Err(RuntimeError {
            message: format!("Could not load dynamic library {}: {}. \
                            Make sure all the dependencies are present in the system and the search paths.", &binary_path.to_str().unwrap(), lib.err().unwrap())
        });
    }
    lib
}
