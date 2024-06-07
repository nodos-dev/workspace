use std::fs;
use std::path::{Path, PathBuf};
use zip::ZipArchive;
use crate::nosman::command::CommandError;

pub fn download_and_extract(url: &str, target: &PathBuf) -> Result<(), CommandError> {
    let mut tmpfile = tempfile::tempfile().expect("Failed to create tempfile");
    reqwest::blocking::get(url)
    .expect(format!("Failed to fetch {}", url).as_str()).copy_to(&mut tmpfile)
    .expect(format!("Failed to write to {:?}", tmpfile).as_str());

    let mut archive = ZipArchive::new(tmpfile)?;
    fs::create_dir_all(target.clone())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = Path::new(&target.clone()).join(file.name());

        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}