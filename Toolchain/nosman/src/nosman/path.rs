use std::path::{PathBuf};
use crate::nosman::constants;

pub fn get_rel_path_based_on(path: &PathBuf, base: &PathBuf) -> PathBuf {
    pathdiff::diff_paths(dunce::canonicalize(path).unwrap(), base).unwrap()
}

pub fn get_module_manifest_file(path: &PathBuf, extension: &str) -> Result<Option<PathBuf>, String> {
    // Find a *.nosman file in the directory
    // If there are multiple, return an error
    let mut manifest_files = vec![];
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == extension {
                    manifest_files.push(path);
                }
            }
        }
    }
    if manifest_files.len() == 0 {
        return Ok(None);
    }
    if manifest_files.len() > 1 {
        return Err(format!("Multiple manifest files found in {}", path.display()));
    }
    Ok(Some(manifest_files[0].clone()))
}

pub fn get_plugin_manifest_file(path: &PathBuf) -> Result<Option<PathBuf>, String> {
    get_module_manifest_file(path, constants::PLUGIN_MANIFEST_FILE_EXT)
}

pub fn get_subsystem_manifest_file(path: &PathBuf) -> Result<Option<PathBuf>, String> {
    get_module_manifest_file(path, constants::SUBSYSTEM_MANIFEST_FILE_EXT)
}
