use std::{fmt, fs};
use std::fmt::Display;
use std::path::PathBuf;
use inquire::Text;
use colored::Colorize;
use crate::nosman::common::{get_nodos_version, NODOS_1_4};
use crate::nosman::constants;
use crate::nosman::index::SemVer;
use crate::nosman::package::LocalPackageEntry;
use crate::nosman::path::get_rel_path_based_on;
use crate::nosman::workspace::Workspace;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct NodeDefinition {
    pub class_name: String,
    pub defined_in: PathBuf,
    pub index: usize,
    pub json: serde_json::Value,
    pub owner: LocalPackageEntry,
}

impl Display for NodeDefinition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.owner.info.id, self.defined_in.display())
    }
}

#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub package: LocalPackageEntry,
}

impl PluginEntry {
    pub fn new(package: LocalPackageEntry) -> Result<Self, String> {
        if !package.package_type.is_plugin() {
            return Err(format!("Package {} is not a plugin", package.info.id.name));
        }
        Ok(PluginEntry { package })
    }

    pub fn get_node_definitions(&self, include_nodes_folder: bool) -> Vec<NodeDefinition> {
        // Read module manifest file as JSON, and read node definition files
        let manifest_json = self.package.read_manifest();
        let node_defs_rel_paths_opt = manifest_json["node_definitions"].as_array();
        let mut node_defs_rel_paths = vec![];
        let mut node_definitions = vec![];

        // If include_nodes_folder is true, add .nosnode files in Nodes folder.
        if include_nodes_folder {
            // Find .nosnode files under Nodes/ folder
            let nodes_dir = self.package.get_package_root().join("Nodes");
            if nodes_dir.exists() {
                for entry in fs::read_dir(&nodes_dir).unwrap_or_else(|e| panic!("Failed to read Nodes directory {:?}: {}", nodes_dir, e)) {
                    let entry = entry.unwrap();
                    let path = entry.path();
                    if path.is_file() && path.extension().map_or(false, |ext| ext == constants::NODE_DEFINITION_FILE_EXT) {
                        let rel_path = get_rel_path_based_on(&path.canonicalize().unwrap(), &self.package.get_package_root());
                        let rel_path_str = rel_path.to_string_lossy().to_string();
                        node_defs_rel_paths.push(rel_path_str);
                    }
                }
            }
        }

        if node_defs_rel_paths_opt.is_some() {
            node_defs_rel_paths.extend(
                node_defs_rel_paths_opt.unwrap()
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
            );
        }

        for node_defs_rel_path in node_defs_rel_paths {
            let node_defs_path = self.package.get_package_root().join(node_defs_rel_path.as_str());
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
                if !curr_class_name.starts_with(self.package.info.id.name.as_str()) {
                    curr_class_name = format!("{}.{}", self.package.info.id.name, curr_class_name);
                }
                node_definitions.push(NodeDefinition {
                    class_name: curr_class_name.clone(),
                    defined_in: node_defs_path.clone(),
                    index,
                    json: node_defs.clone(),  
                    owner: self.package.clone(),
                });
            }
        }
        node_definitions
    }

    // Iterates over all node definitions and finds the one with the given class name.
    // Don't call this function multiple times if you have many nodes, as it will read and parse node definition files every time. Instead, call get_node_definitions once and find the node definition from the returned list.
    pub fn get_node_definition(&self, class_name: &str, nodos_version: &Option<SemVer>) -> Option<NodeDefinition>{
        let include_nodes_folder = nodos_version.as_ref().map_or(false, |v| v >= &NODOS_1_4);
        let node_defs = self.get_node_definitions(include_nodes_folder);
        return node_defs.into_iter().find(|def| def.class_name == class_name);
    }

    pub fn remove_node_definition(&self, node_class_name: &String, nodos_version: Option<SemVer>) -> Result<(), String> {
        let node_def = self.get_node_definition(node_class_name.as_str(), &nodos_version);
        if node_def.is_none() {
            return Err(format!("Node class {} not found in plugin {}", node_class_name, self.package));
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
            let mut manifest_json = self.package.read_manifest();
            if !manifest_json["node_definitions"].is_null() {
                let node_defs_rel_paths = manifest_json["node_definitions"].as_array_mut().unwrap();
                // Remove the node definition file from the manifest
                node_defs_rel_paths.retain(|path| {
                    let path_obj = self.package.get_package_root().join(PathBuf::from(path.as_str()
                        .unwrap_or_else(|| panic!("Failed to convert path to string: {}", path))));
                    let path_obj = dunce::canonicalize(&path_obj).unwrap_or_else(|e| panic!("Failed to canonicalize path: {} {}", path_obj.display(), e));
                    path_obj != node_def_path
                });
                // Write back to manifest
                let manifest_str = serde_json::to_string_pretty(&manifest_json).unwrap_or_else(|e| panic!("Failed to serialize module manifest: {}", e));
                fs::write(&self.package.manifest_path, manifest_str).unwrap_or_else(|e| panic!("Failed to write module manifest file {}: {}", self.package.manifest_path.display(), e));
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
        println!("{}", format!("Adding a node '{}' to plugin: {}", node_class_name, self.package).green());
        let node_class_name = if node_class_name.starts_with(self.package.info.id.name.as_str()) {
            node_class_name.clone()
        }
        else {
            format!("{}.{}", self.package.info.id.name, node_class_name)
        };
        let node_def = self.get_node_definition(&node_class_name, &nodos_version);
        if node_def.is_some() {
            return Err(format!("Node class {} already exists in plugin {}", node_class_name, self.package));
        }
        let display_name = display_name.unwrap_or_else(|| {
            let default_display_name = node_class_name.strip_prefix(format!("{}.", &self.package.info.id.name).as_str())
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
        let mut manifest_json = self.package.read_manifest();
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


        let out_node_defs_file = format!("Nodes/{}", node_class_name.strip_prefix(format!("{}.", &self.package.info.id.name).as_str()).unwrap_or_else(|| &node_class_name));
        println!("Node definition file: {}", out_node_defs_file);
        let node_def_path = PathBuf::from(&out_node_defs_file).with_extension(node_def_file_ext).to_path_buf();
        node_defs_rel_paths.push(serde_json::Value::String(node_def_path.to_str()
            .unwrap_or_else(|| panic!("Failed to convert path to string: {}", node_def_path.display())).to_string()));
        let out_node_defs_path = self.package.get_package_root().join(&out_node_defs_file);
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
            fs::write(&self.package.manifest_path, manifest_str).unwrap_or_else(|e| panic!("Failed to write module manifest file {}: {}", self.package.manifest_path.display(), e));
        }
        Ok(())
    }
}