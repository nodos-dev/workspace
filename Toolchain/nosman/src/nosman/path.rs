use std::path::{PathBuf};

pub fn get_rel_path_based_on(path: &PathBuf, base: &PathBuf) -> PathBuf {
    pathdiff::diff_paths(dunce::canonicalize(path).unwrap(), base).unwrap()
}
