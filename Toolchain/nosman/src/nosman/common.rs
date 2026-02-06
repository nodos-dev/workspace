use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Mutex, OnceLock};
use colored::Colorize;
use include_dir::Dir;
use indicatif::ProgressBar;
use inquire::Confirm;
use zip::ZipArchive;
use serde_json::Value;
use crate::nosman::command::CommandError;
use crate::nosman::command::sdk_info::get_engine_sdk_infos;
use crate::nosman::index::SemVer;
use crate::nosman::workspace::Workspace;

pub type PromptHandler = dyn Fn(&str, bool, bool) -> bool + Send + Sync + 'static;
static PROMPT_HANDLER: OnceLock<Mutex<Option<Box<PromptHandler>>>> = OnceLock::new();

pub fn set_prompt_handler(handler: Option<Box<PromptHandler>>) {
    let slot = PROMPT_HANDLER.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap();
    *guard = handler;
}

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

pub fn copy_include_dir_recursive(
    src: &Dir,
    dest: &Path,
    mut modify: Option<&mut dyn FnMut(&mut String)>,
) -> Result<(), CommandError> {
    let mut stack: Vec<&Dir> = vec![src];
    while let Some(dir) = stack.pop() {
        let target_dir = dest.join(dir.path().strip_prefix(src.path()).unwrap());
        for entry in dir.entries() {
            if let Some(d) = entry.as_dir() {
                stack.push(d);
                fs::create_dir_all(target_dir.join(entry.path().file_name().unwrap()))?;
            } else if let Some(file) = entry.as_file() {
                let target_path = target_dir.join(entry.path().file_name().unwrap());
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                if let Some(modify) = modify.as_deref_mut() {
                    let mut content = file
                        .contents_utf8()
                        .ok_or(CommandError::Runtime {
                            message: format!(
                                "Non-UTF8 file found in embedded dir: {}",
                                entry.path().display()
                            ),
                        })?
                        .to_string();
                    modify(&mut content);
                    fs::write(target_path, content)?;
                } else {
                    fs::write(target_path, file.contents())?;
                }
            }
        }
    }
    Ok(())
}

pub fn copy_dir_recursive(
    src: &Path,
    dest: &Path,
    mut modify: Option<&mut dyn FnMut(&Path, &mut String)>,
    skip: Option<&dyn Fn(&Path) -> bool>,
) -> Result<(), CommandError> {
    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rel = dir.strip_prefix(src).unwrap_or(&dir);
        let target_dir = dest.join(rel);
        fs::create_dir_all(&target_dir)?;
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                if let Some(skip) = skip {
                    if skip(&path) {
                        continue;
                    }
                }
                let rel_path = path.strip_prefix(src).unwrap_or(&path);
                let target_path = dest.join(rel_path);
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                if let Some(modify) = modify.as_deref_mut() {
                    let mut content = fs::read_to_string(&path).map_err(|e| CommandError::IO {
                        file: path.to_string_lossy().to_string(),
                        message: format!("Failed to read template file: {}", e),
                    })?;
                    modify(&path, &mut content);
                    fs::write(&target_path, content)?;
                } else {
                    fs::copy(&path, &target_path)?;
                }
            }
        }
    }
    Ok(())
}

pub fn ask(question: &str, default: bool, dont_ask: bool) -> bool {
    if dont_ask {
        return default;
    }
    if let Some(slot) = PROMPT_HANDLER.get() {
        if let Some(handler) = slot.lock().unwrap().as_ref() {
            return handler(question, default, dont_ask);
        }
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

pub fn read_file_contents(file: &PathBuf, tag: &str) -> Result<String, CommandError> {
    fs::read_to_string(file)
        .map_err(|e| CommandError::IO {
            file: file.to_string_lossy().to_string(),
            message: format!("Failed to read {} file: {}", tag, e),
        })
}

pub fn collect_files_recursive<F>(dir: &Path, predicate: F) -> Vec<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if predicate(&path) {
                out.push(path);
            }
        }
    }
    out
}

pub static NODOS_1_4: SemVer = SemVer { major: 1, minor: Some(4), patch: None, build_number: None };
pub static NODOS_1_3: SemVer = SemVer { major: 1, minor: Some(3), patch: None, build_number: None };

pub static SUPPORTED_NODOS_VERSIONS: [&'static SemVer; 2] = [
    &NODOS_1_4,
    &NODOS_1_3,
];

pub static DEFAULT_NODOS_VERSION_INDEX: usize = 0;

pub fn is_nodos_version_supported(version: &SemVer) -> bool {
    SUPPORTED_NODOS_VERSIONS.iter().any(|v| *v == version)
}

pub fn get_nodos_version(workspace: &Workspace, nodos_version: &Option<SemVer>) -> Result<SemVer, String> {
    let mut selected_version = SUPPORTED_NODOS_VERSIONS[DEFAULT_NODOS_VERSION_INDEX].clone();
    if let Some(nodos_ver) = nodos_version {
        selected_version = nodos_ver.clone();
        if !is_nodos_version_supported(&selected_version) {
            return Err(format!("Unsupported Nodos version: {}", selected_version.to_string()));
        }
    } else if workspace.ready() {
        let engines = get_engine_sdk_infos(workspace);
        if let Ok(engines) = engines {
            let mut major_minors = HashSet::<SemVer>::new();
            for engine in engines {
                if let Some(semver) = SemVer::parse_from_str(engine.version.as_str()) {
                    major_minors.insert(semver);
                }
            }
            if major_minors.len() > 1 {
                // Multiple versions found, select the latest one:
                let mut versions: Vec<SemVer> = major_minors.into_iter().collect();
                versions.sort_by(|a, b| {
                    a.cmp(&b) // Descending order
                });
                selected_version = versions[0].clone();
            } else if major_minors.len() == 1 {
                // Only one version found, use it
                selected_version = major_minors.into_iter().next().unwrap();
            }
        }
    }
    Ok(selected_version)
}
