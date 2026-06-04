use std::collections::HashMap;
use std::path::{PathBuf};
use crate::nosman::index::{PackageType};

pub fn get_rel_path_based_on(path: &PathBuf, base: &PathBuf) -> PathBuf {
    pathdiff::diff_paths(dunce::canonicalize(path).unwrap_or_else(|_| { panic!("Failed to canonicalize path {}", path.display()) }),
                         base).unwrap()
}

pub (crate) static MANIFEST_EXT_TO_PACKAGE_TYPE: phf::Map<&'static str, PackageType> = phf::phf_map! {
    "nosplugin" => PackageType::Plugin,
    "noscfg" => PackageType::Plugin,
    "nossys" => PackageType::Subsystem,
    "nospackage" => PackageType::Generic,
    "nosengine" => PackageType::Nodos,
};

pub fn get_package_manifest_file(folder: &PathBuf) -> Result<Option<(PackageType, PathBuf)>, String> {
    let mut found_manifests: HashMap<PackageType, PathBuf> = HashMap::new();
    let mut files = vec![];
    for entry in std::fs::read_dir(folder).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if let Some(ext_str) = ext.to_str() {
                    if let Some(package_type) = MANIFEST_EXT_TO_PACKAGE_TYPE.get(ext_str).cloned() {
                        if found_manifests.contains_key(&package_type) {
                            return Err(format!("Multiple {} files found in {}", ext_str, folder.display()));
                        }
                        found_manifests.insert(package_type, path.clone());
                        files.push(path);
                    }
                }
            }
        }
    }
    if files.is_empty() {
        return Ok(None);
    }
    if found_manifests.len() > 1 {
        let mut types: Vec<String> = found_manifests.keys().map(|k| format!("{:?}", k)).collect();
        types.sort();
        return Err(format!("Multiple manifest files found in {}: {}", folder.display(), types.join(", ")));
    }
    Ok(Some(found_manifests.into_iter().next().unwrap()))
}

pub fn get_default_engines_dir(workspace: &PathBuf) -> PathBuf {
    workspace.join("Engine")
}

