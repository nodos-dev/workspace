use serde::{Deserialize, Serialize};
use std::{fmt, fs, ptr};
use std::ffi::{CString, OsString};
use std::fmt::Display;
use std::os::raw::c_int;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(unix)]
use std::env;
use std::path::PathBuf;
use std::time::Duration;
use colored::Colorize;
use inquire::Text;
use libloading::Library;
use crate::nosman::command::{CommandError, CommandResult};
use crate::nosman::command::CommandError::{InvalidArgument, Runtime};
use crate::nosman::{common, constants, extensions};
use crate::nosman::common::{get_progress_bar};
use crate::nosman::extensions::{CNosArg, CNosCommand, CNosRunCommandParams, NosCommand, NosCommandDesc};
use crate::nosman::index::{ModuleType};
use crate::nosman::path::{get_plugin_manifest_file, get_rel_path_based_on, get_subsystem_manifest_file};
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

#[derive(Serialize, Deserialize, Debug, Hash, Clone, Eq, PartialEq)]
pub struct InstalledModule {
    pub info: ModuleInfo,
    #[serde(alias = "config_path")]
    pub manifest_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_include_folder: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub type_schema_files: Vec<PathBuf>,
    pub module_type: ModuleType,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
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
    pub fn new(workspace: &Workspace, path: PathBuf, register_commands: bool) -> Result<InstalledModule, String> {
        let mut installed_module = InstalledModule {
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
            manifest_path: path.clone(),
            public_include_folder: None,
            type_schema_files: Vec::new(),
            module_type: ModuleType::Plugin,
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
        let module = res.unwrap();
        installed_module.info = serde_json::from_value(module["info"].clone()).unwrap_or_else(|e| panic!("Failed to parse module info from {:?}: {}", path, e));

        // Check custom_types field
        if let Some(custom_types) = module["custom_types"].as_array() {
            for custom_type_file in custom_types {
                let type_file = abs_path.parent().unwrap().join(custom_type_file.as_str().unwrap());
                if !type_file.exists() {
                    return Err(format!("Module {} ({}) references a non-existent data schema file: {}", installed_module.info.id.name, path.display(), type_file.display()).as_str().red().to_string());
                }
                installed_module.type_schema_files.push(get_rel_path_based_on(&type_file.canonicalize().unwrap(), &workspace.root));
            }
        }

        // Check include folder
        if abs_path.parent().unwrap().join("Include").exists() {
            installed_module.public_include_folder = Some(get_rel_path_based_on(&abs_path.parent().unwrap().join("Include").canonicalize().unwrap(), &workspace.root));
        }
        installed_module.module_type = get_module_type_from_manifest_file_path(&abs_path).unwrap();
        if register_commands {
            installed_module.register_commands(&workspace);
        }
        Ok(installed_module)
    }
    pub fn get_module_dir(&self) -> PathBuf {
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
    pub fn get_node_definition(&self, class_name: &str) -> Option<NodeDefinition> {
        if self.module_type != ModuleType::Plugin {
            return None;
        }
        // Read module manifest file as JSON, and read node definition files
        let manifest_json = self.read_manifest();
        let node_defs_rel_paths = manifest_json["node_definitions"].as_array()?;
        for node_defs_rel_path in node_defs_rel_paths {
            let node_defs_path = self.get_module_dir().join(node_defs_rel_path.as_str()?);
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
                        node_defs_json: node_defs.clone(),
                        owner: self.clone(),
                    });
                }
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
        let node_defs_file_content = serde_json::to_string_pretty(&node_def.node_defs_json).unwrap_or_else(|e| panic!("Failed to serialize node definitions at {}: {}", node_def.defined_in.display(), e));
        fs::write(&node_def.defined_in, node_defs_file_content).unwrap_or_else(|e| panic!("Failed to write node definitions file {}: {}", node_def.defined_in.display(), e));
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

        let mut manifest_json = self.read_manifest();
        let node_defs_rel_paths = manifest_json["node_definitions"].as_array_mut().unwrap_or_else(|| panic!("Missing 'node_definitions' field in module manifest file {}", self.manifest_path.display()));
        let out_node_defs_file = Text::new("Node definitions file:")
            .with_default(format!("Config/{}", node_class_name.strip_prefix(format!("{}.", &self.info.id.name).as_str()).unwrap()).as_str()).prompt()
            .unwrap_or_else(|e| panic!("Failed to get node definitions file: {}", e));
        let node_def_path = PathBuf::from(&out_node_defs_file).with_extension(constants::NODE_DEF_FILE_EXT).to_path_buf();
        node_defs_rel_paths.push(serde_json::Value::String(node_def_path.to_str()
            .unwrap_or_else(|| panic!("Failed to convert path to string: {}", node_def_path.display())).to_string()));
        let out_node_defs_path = self.get_module_dir().join(&out_node_defs_file);
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
        // If no .nosdef extension, add it
        let out_node_defs_path = if out_node_defs_path.extension().is_none() {
            out_node_defs_path.with_extension(constants::NODE_DEF_FILE_EXT)
        } else {
            out_node_defs_path
        };
        fs::write(&out_node_defs_path, node_defs_str).unwrap_or_else(|e| panic!("Failed to write node definitions file {}: {}", out_node_defs_path.display(), e));
        // Update manifest file
        let manifest_str = serde_json::to_string_pretty(&manifest_json).unwrap_or_else(|e| panic!("Failed to serialize module manifest: {}", e));
        fs::write(&self.manifest_path, manifest_str).unwrap_or_else(|e| panic!("Failed to write module manifest file {}: {}", self.manifest_path.display(), e));
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
    pub fn run_command(&self, workspace: &Workspace, command_name: &str, params: NosCommand) -> CommandResult {
        let lib = load_installed_module(&self, workspace)?;
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
        let res = InstalledModule::new(workspace, self.manifest_path.clone(), 
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

pub fn get_module_type_from_manifest_file_path(file_path: &PathBuf) -> Option<ModuleType> {
    if file_path.extension()?.to_str()? == constants::SUBSYSTEM_MANIFEST_FILE_EXT {
        Some(ModuleType::Subsystem)
    }
    else if file_path.extension()?.to_str()? == constants::PLUGIN_MANIFEST_FILE_EXT {
        Some(ModuleType::Plugin)
    } else {
        None
    }
}

pub fn get_module_manifest_and_type(dir: &PathBuf) -> Result<Option<(ModuleType, PathBuf)>, CommandError> {
    let res = get_plugin_manifest_file(&dir);
    if res.is_err() {
        return Err(InvalidArgument { message: res.err().unwrap() });
    }
    let plugin_manifest_file = res.unwrap();
    let res = get_subsystem_manifest_file(&dir);
    if res.is_err() {
        return Err(InvalidArgument { message: res.err().unwrap() });
    }
    let subsystem_manifest_file = res.unwrap();
    if plugin_manifest_file.is_some() && subsystem_manifest_file.is_some() {
        return Err(InvalidArgument { message: format!("Multiple module manifest files found in {}", dir.display()) });
    }

    let mut opt_module_type = None;
    if plugin_manifest_file.is_some() {
        opt_module_type = Some(ModuleType::Plugin);
    } else if subsystem_manifest_file.is_some() {
        opt_module_type = Some(ModuleType::Subsystem);
    }
    let opt_manifest_file = plugin_manifest_file.or(subsystem_manifest_file);
    if let Some(manifest) = opt_manifest_file {
        return Ok(Some((opt_module_type.unwrap(), manifest)))
    }
    Ok(None)
}

pub fn get_module_manifests(folder: &PathBuf, silent: bool) -> Vec<(ModuleType, PathBuf)> {
    let pb = get_progress_bar(silent);
    pb.enable_steady_tick(Duration::from_millis(100));

    pb.set_message(format!("Looking for Nodos modules in {}", folder.to_str().unwrap_or_else(|| panic!("Non-UTF-8 path: {}", folder.display()))).to_string());
    let res = get_module_manifest_file_in_folder(&folder);
    if res.is_ok() {
        if let Some((ty, mpath)) = res.unwrap() {
            return vec![(ty, mpath)];
        }
    }

    let patterns = &[
        format!("*.{{{},{}}}", constants::SUBSYSTEM_MANIFEST_FILE_EXT, constants::PLUGIN_MANIFEST_FILE_EXT),
        "!**/.git/**".to_string()
    ];
    let walker = globwalk::GlobWalkerBuilder::from_patterns(folder, patterns)
        .file_type(globwalk::FileType::FILE)
        .build()
        .unwrap_or_else(|e| panic!("Failed to glob dirs: {:?}: {}", patterns, e));
    let mut module_manifest_files = vec![];
    for entry in walker {
        match entry {
            Ok(entry) => {
                let path = entry.path().to_path_buf();
                // If multiple manifest files are found in the same folder, we will skip this folder
                let parent = path.parent().unwrap_or_else(|| panic!("No parent folder found for path: {}", path.display())).to_path_buf();
                let res = get_module_manifest_file_in_folder(&parent);
                if let Ok(res) = res {
                    if let Some((ty, mpath)) = res {
                        module_manifest_files.push((ty, mpath));
                    }
                }
            }
            Err(e) => {
                pb.println(format!("Error while walking: {}", e));
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
            return Err(Runtime { message: format!("Failed to load dynamic library: {}", res.err().unwrap()) });
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
            return Err(Runtime { message: format!("Failed to set default DLL directories: {}", err) });
        }
        let mut dll_cookies = vec![];
        for lib_dir in additional_search_paths {
            if !lib_dir.exists() {
                println!("{}", format!("Warning: DLL search path {} does not exist", lib_dir.display()).yellow().to_string());
                continue;
            }
            let lib_dir_canonical = dunce::canonicalize(&lib_dir).unwrap_or_else(|e| panic!("Failed to canonicalize path {:?}: {}", lib_dir, e));
            if verbose {
                println!("\tAdding DLL search path: {}", lib_dir_canonical.display());
            }
            let wdir: Vec<u16> = lib_dir_canonical.as_os_str().encode_wide().chain(Some(0)).collect();
            let cookie = AddDllDirectory(wdir.as_ptr());
            if cookie.is_null() {
                let err = std::io::Error::last_os_error();
                return Err(Runtime { message: format!("Failed to add DLL search path {}: {}", lib_dir_canonical.display(), err) });
            }
            dll_cookies.push(cookie);
        }
        let res = Library::new(&binary_path);
        for cookie in dll_cookies {
            RemoveDllDirectory(cookie);
        }
        if res.is_err() {
            return Err(Runtime { message: format!("Failed to load dynamic library: {}", res.err().unwrap()) });
        }
        Ok(res.unwrap())
    }
}

pub fn load_installed_module(module: &InstalledModule, workspace: &Workspace) -> Result<Library, CommandError> {
    let path = module.get_abs_manifest_path(workspace);
    let manifest_file_contents = common::read_or_fail(&path, "module manifest");
    let manifest: serde_json::Value = serde_json::from_str(&manifest_file_contents).unwrap_or_else(|e| panic!("Failed to parse module manifest file {}: {}", path.display(), e));
    load_module(false, manifest, module.get_abs_manifest_path(workspace).parent().unwrap().to_path_buf(), workspace)
}

pub fn load_module(verbose: bool, manifest: serde_json::Value, manifest_file_parent: PathBuf, workspace: &Workspace) -> Result<Library, CommandError> {
    let binary_path = manifest["binary_path"].as_str();
    if binary_path.is_none() {
        return Err (InvalidArgument {message: "Module manifest does not specify a binary path".to_string() })
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
            let dep_manifest_file_contents = common::read_or_fail(&dep_manifest_file_path, "dependency manifest");
            let dep_manifest: serde_json::Value = serde_json::from_str(&dep_manifest_file_contents).unwrap_or_else(|e| panic!("Failed to parse dependency manifest file {}: {}", dep_manifest_file_path.display(), e));
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
        return Err(Runtime {
            message: format!("Could not load dynamic library {}: {}. \
                            Make sure all the dependencies are present in the system and the search paths.", &binary_path.to_str().unwrap(), lib.err().unwrap())
        });
    }
    lib
}

use clap::{ArgMatches};
pub fn get_dependency_arguments(args: &ArgMatches, allow_any: bool, success: &mut bool) -> Vec<PackageIdentifier>{
    let depss: Vec<&String> = args.get_many::<String>("dependency").unwrap_or_default().collect();
    let mut deps = Vec::new();
    for dep in depss {
        let mut parts: Vec<&str> = dep.split('-').collect();
        if parts.len() == 1 && allow_any{
            *success = true;
            parts.push("any");
        }
        if parts.len() == 2 {
            *success = true;
        }
        else{
            *success = false;
            println!("Invalid dependency format: {}", dep);
        }
        deps.push(PackageIdentifier {
            name: parts[0].to_string(),
            version: parts[1].to_string(),
        });
    }
    *success = true;
    deps
}
