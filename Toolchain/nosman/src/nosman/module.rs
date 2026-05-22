use crate::nosman::command::CommandError::{InvalidArgument, Runtime};
use crate::nosman::command::CommandError;
use crate::nosman::index::PackageType;
use crate::nosman::platform::get_host_platform;
use crate::nosman::workspace::Workspace;
use crate::nosman::common;
#[cfg(target_os = "windows")]
use colored::Colorize;
use libloading::Library;
#[cfg(unix)]
use std::env;
use std::ffi::OsString;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use crate::nosman::package::{LocalPackageEntry, PackageIdentifier};
use clap::ArgMatches;

pub fn load_dylib_with_search_paths(verbose: bool, binary_path: &OsString, additional_search_paths: Vec<PathBuf>) -> Result<Library, CommandError> {
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
            res = Library::new(binary_path)
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
        use winapi::um::libloaderapi::{AddDllDirectory, RemoveDllDirectory, SetDefaultDllDirectories};
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
        let res = Library::new(binary_path);
        for cookie in dll_cookies {
            RemoveDllDirectory(cookie);
        }
        if res.is_err() {
            return Err(Runtime { message: format!("Failed to load dynamic library: {}", res.err().unwrap()) });
        }
        Ok(res.unwrap())
    }
}

pub fn load_module_from_manifest(package: &LocalPackageEntry, workspace: &Workspace) -> Result<Library, CommandError> {
    let path = package.get_abs_manifest_path(workspace);
    let manifest_file_contents = common::read_file_contents(&path, "package manifest")?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_file_contents).unwrap_or_else(|e| panic!("Failed to parse package manifest file {}: {}", path.display(), e));
    load_module(false, &package.package_type, &manifest, package.get_abs_manifest_path(workspace).parent().unwrap().to_path_buf(), workspace)
}

fn generate_binary_name_from_package_name(package_name: &str) -> Option<String> {
    let mut parts = package_name.split('.').filter(|part| !part.is_empty());
    let first = parts.next()?;
    let mut generated = String::from(first);
    for part in parts {
        let mut chars = part.chars();
        let first_char = chars.next()?;
        generated.push(first_char.to_ascii_uppercase());
        generated.extend(chars);
    }
    Some(generated)
}

pub fn get_manifest_binary_path(package_type: &PackageType, manifest: &serde_json::Value) -> Result<String, CommandError> {
    let key = match package_type {
        PackageType::Plugin => { "binary_path" }
        PackageType::Subsystem => { "binary_path" }
        PackageType::Generic => { "cli_extension_bin_path" },
        _ => {
            return Err(InvalidArgument { message: format!("Unsupported package type: {:?}", package_type) });
        }
    };
    if let Some(path) = manifest[key].as_str() {
        return Ok(path.to_string());
    }
    if *package_type == PackageType::Plugin {
        let package_name = manifest["info"]["id"]["name"].as_str()
            .ok_or(InvalidArgument { message: "Package manifest does not specify a binary path or package name".to_string() })?;
        let generated_name = generate_binary_name_from_package_name(package_name)
            .ok_or(InvalidArgument { message: format!("Failed to generate binary path from package name {}", package_name) })?;
        return Ok(format!("Binaries/{}", generated_name));
    }
    Err(InvalidArgument {message: "Package manifest does not specify a binary path".to_string() })
}

pub fn get_resolved_binary_path(package_type: &PackageType, manifest: &serde_json::Value, module_dir: &PathBuf) -> Result<PathBuf, CommandError> {
    let binary_path = get_manifest_binary_path(package_type, manifest)?;
    let binary_path = module_dir.join(binary_path);
    let host_platform = get_host_platform();
    Ok(binary_path.with_extension(
        if host_platform.os == "windows" { "dll" }
        else if host_platform.os == "macos" { "dylib" }
        else { "so" }
    ))
}

pub fn load_module(verbose: bool, package_type: &PackageType, manifest: &serde_json::Value, manifest_file_parent: PathBuf, workspace: &Workspace) -> Result<Library, CommandError> {
    let binary_path = get_resolved_binary_path(package_type, manifest, &manifest_file_parent)?;
    let module_dir = manifest_file_parent;
    let mut additional_search_paths: Vec<PathBuf> = Vec::new();
    // Search the binary's own directory, so plugins that ship native
    // dependencies (e.g. a bundled SDK runtime DLL) next to their binary load.
    if let Some(binary_dir) = binary_path.parent() {
        additional_search_paths.push(binary_dir.to_path_buf());
    }
    let binary_path = binary_path.into_os_string();
    for path_str in manifest["additional_search_paths"].as_array().unwrap_or(&vec![]).iter() {
        let path = module_dir.join(path_str.as_str().unwrap());
        additional_search_paths.push(path);
    }
    // Add search paths of dependencies
    for dep in manifest["info"]["dependencies"].as_array().unwrap_or(&vec![]) {
        let dep_name = dep["name"].as_str().unwrap();
        let dep_version = dep["version"].as_str().unwrap();
        let dep_res = workspace.get_latest_local_package_for_version(dep_name, dep_version);
        if let Ok(installed_module) = dep_res {
            let dep_manifest_file_path = workspace.root.join(&installed_module.manifest_path);
            let dep_manifest_file_contents = common::read_file_contents(&dep_manifest_file_path, "dependency manifest")?;
            let dep_manifest: serde_json::Value = serde_json::from_str(&dep_manifest_file_contents).unwrap_or_else(|e| panic!("Failed to parse dependency manifest file {}: {}", dep_manifest_file_path.display(), e));
            for path_str in dep_manifest["additional_search_paths"].as_array().unwrap_or(&vec![]) {
                let module_dir = dep_manifest_file_path.parent().unwrap();
                let path = module_dir.join(path_str.as_str().unwrap());
                additional_search_paths.push(path);
            }
        }
    }
    // Load the dynamic library
    let lib = load_dylib_with_search_paths(verbose, &binary_path, additional_search_paths);
    if lib.is_err() {
        return Err(Runtime {
            message: format!("Could not load dynamic library {}: {}. \
                            Make sure all the dependencies are present in the system and the search paths.", &binary_path.to_str().unwrap(), lib.err().unwrap())
        });
    }
    lib
}

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

