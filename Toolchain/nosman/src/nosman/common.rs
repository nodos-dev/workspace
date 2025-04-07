use std::fs;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::Output;
use colored::Colorize;
use indicatif::ProgressBar;
use inquire::Confirm;
use zip::ZipArchive;
use serde_json::Value;
use crate::nosman::command::CommandError;

pub fn download_and_extract(url: &str, target: &PathBuf) -> Result<(), CommandError> {
    let mut tmpfile = tempfile::tempfile().expect("Failed to create tempfile");
    reqwest::blocking::get(url)
    .unwrap_or_else(|e| panic!("Failed to fetch {}: {}", url, e)).copy_to(&mut tmpfile)
    .unwrap_or_else(|e| panic!("Failed to write to {:?}: {}", tmpfile, e));

    tmpfile.seek(std::io::SeekFrom::Start(0)).expect("Failed to seek to start of tempfile");

    // If tar.gz, use flate2 to extract
    if url.ends_with(".tar.gz") {
        #[cfg(unix)]
        {
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tmpfile));
            fs::create_dir_all(target.clone())?;
            archive.unpack(&target)?;
            return Ok(());
        }
    }
    
    let mut archive = ZipArchive::new(tmpfile)?;
    fs::create_dir_all(target.clone())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let filename = file.name();
        // It might contain \, so convert this to POSIX compatible path
        let filename = filename.replace("\\", "/");
        let outpath = Path::new(&target).join(filename);

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

pub fn check_file_contents_same(path1: &PathBuf, path2: &PathBuf) -> bool {
    // Efficiently compare file contents
    let mut file1 = File::open(path1).unwrap_or_else(|e| panic!("Failed to open {:?}: {}", path1, e));
    let mut file2 = File::open(path2).unwrap_or_else(|e| panic!("Failed to open {:?}: {}", path2, e));
    let mut buf1 = [0; 1024];
    let mut buf2 = [0; 1024];
    let opt_f1_md = file1.metadata();
    let opt_f2_md = file2.metadata();
    if opt_f1_md.is_err() || opt_f2_md.is_err() {
        return false;
    }
    let f1_md = opt_f1_md.unwrap();
    let f2_md = opt_f2_md.unwrap();
    if f1_md.len() != f2_md.len() {
        return false;
    }
    loop {
        let n1 = file1.read(&mut buf1).unwrap_or_else(|e| panic!("Failed to read {:?}: {}", path1, e));
        let n2 = file2.read(&mut buf2).unwrap_or_else(|e| panic!("Failed to read {:?}: {}", path2, e));
        if n1 != n2 || buf1 != buf2 {
            return false;
        }
        if n1 == 0 {
            break;
        }
    }
    true
}

pub fn ask(question: &str, default: bool, dont_ask: bool) -> bool {
    if dont_ask {
        return default;
    }
    loop {
        let res = Confirm::new(question)
            .with_default(default)
            .prompt();
        if let Ok(result) = res {
            return result;
        } else if let Err(e) = res {
            eprintln!("{}", e);
        }
    }
}

pub fn run_if_not(dry_run: bool, verbose: bool, cmd: &mut std::process::Command) -> Option<Output> {
    if dry_run {
        println!("{}", format!("Would run: {:?}", cmd).cyan());
        None
    } else {
        if verbose {
            println!("{}", format!("Running: {:?}", cmd).cyan());
        }
        let res = cmd.output();
        if verbose && res.is_ok() {
            let output = res.as_ref().unwrap();
            println!("{}:\n{}", if output.status.success() { "stdout" } else { "stderr" },
                     String::from_utf8_lossy(if output.status.success() { &output.stdout } else { &output.stderr }));
        }
        Some(res.unwrap_or_else(|_| panic!("Failed to run command {:?}", cmd)))
    }
}

pub fn get_hostname() -> String {
    let hostname = hostname::get().expect("Failed to get hostname");
    hostname.into_string().expect("Failed to convert hostname to string")
}

pub fn get_progress_bar(silent: bool) -> ProgressBar{
    if silent {
        ProgressBar::hidden()
    } else {
        ProgressBar::new_spinner()
    }
}

pub fn get_string<'a>(json: &'a Value, field: &str, file: &Path) -> &'a str {
    json.get(field)
        .unwrap_or_else(|| panic!("{} field not found in {:?}", field, file))
        .as_str()
        .unwrap_or_else(|| panic!("{} field is not a string in {:?}", field, file))
}

pub fn read_or_fail(file: &PathBuf, tag: &str) -> String {
    fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("Failed to read {} file {:?}: {}", tag, file, e))
}