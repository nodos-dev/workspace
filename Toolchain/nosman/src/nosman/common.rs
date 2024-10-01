use std::fs;
use std::fs::File;
use std::io::{Read};
use std::path::{Path, PathBuf};
use std::process::Output;
use inquire::Confirm;
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
    let mut file1 = File::open(path1).expect(format!("Failed to open {:?}", path1).as_str());
    let mut file2 = File::open(path2).expect(format!("Failed to open {:?}", path2).as_str());
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
        let n1 = file1.read(&mut buf1).expect(format!("Failed to read {}", path1.display()).as_str());
        let n2 = file2.read(&mut buf2).expect(format!("Failed to read {}", path2.display()).as_str());
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
        if res.is_err() {
            eprintln!("{}", res.err().unwrap());
        } else {
            return res.unwrap();
        }
    }
}

pub fn run_if_not(dry_run: bool, verbose: bool, cmd: &mut std::process::Command) -> Option<Output> {
    if dry_run {
        println!("Would run: {:?}", cmd);
        None
    } else {
        if verbose {
            println!("Running: {:?}", cmd);
        }
        let res = cmd.output();
        if verbose {
            if res.is_ok() {
                let output = res.as_ref().unwrap();
                println!("{}:\n{}", if output.status.success() { "stdout" } else { "stderr" },
                         String::from_utf8_lossy(if output.status.success() { &output.stdout } else { &output.stderr }));
            }
        }
        Some(res.expect(format!("Failed to run command {:?}", cmd).as_str()))
    }
}

pub fn get_hostname() -> String {
    let hostname = hostname::get().expect("Failed to get hostname");
    hostname.into_string().expect("Failed to convert hostname to string")
}