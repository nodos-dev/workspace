mod support;

use nosman::nosman::command::info::InfoCommand;
use nosman::nosman::command::install::{InstallCommand, InstallFlags};
use nosman::nosman::workspace::{OutputMode, RescanFlags, Workspace};
use std::path::PathBuf;

#[test]
fn info_by_package_and_version() {
    let mut test = workspace!();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");

    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .expect("Failed to install package");

    let query = nosman::nosman::command::info::PackageQuery {
        name: package_name,
        version_prefix: &version,
    };
    let result = InfoCommand {}.run_get_info(&mut test.workspace, Some(&query), false, None);
    assert!(result.is_ok(), "Failed to get package info by name and version");
}

#[test]
fn info_by_manifest_path() {
    let mut test = workspace!();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");

    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .expect("Failed to install package");

    let package = test
        .workspace
        .get_package(package_name, &version)
        .expect("Package not found");
    let manifest_path = test.workspace.root.join(&package.manifest_path);

    let result = InfoCommand {}.run_get_info(
        &mut test.workspace,
        None,
        false,
        Some(manifest_path.to_str().unwrap()),
    );
    assert!(result.is_ok(), "Failed to get package info by manifest path");
}

#[test]
fn info_error_both_package_and_manifest() {
    let mut test = workspace!();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");

    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .expect("Failed to install package");

    let package = test
        .workspace
        .get_package(package_name, &version)
        .expect("Package not found");
    let manifest_path = test.workspace.root.join(&package.manifest_path);

    let query = nosman::nosman::command::info::PackageQuery {
        name: package_name,
        version_prefix: &version,
    };
    let result = InfoCommand {}.run_get_info(
        &mut test.workspace,
        Some(&query),
        false,
        Some(manifest_path.to_str().unwrap()),
    );
    assert!(result.is_err(), "Should fail when both package and manifest are provided");
}

#[test]
fn info_stdout_is_pure_json_when_index_entry_is_stale() {
    let path = uninitialized_workspace!();
    let module_dir = path.join("Module").join("testDummy");
    std::fs::create_dir_all(&module_dir).expect("Failed to create module dir");
    std::fs::write(
        module_dir.join("testDummy.nossys"),
        r#"{ "info": { "id": { "name": "test.sys.dummy", "version": "1.0.0" } }, "binary_path": "Binaries/testDummy" }"#,
    )
    .expect("Failed to write manifest");

    let mut ws = Workspace::from_root(&path);
    ws.push_output_mode(OutputMode::Silent);
    ws.rescan(RescanFlags::ScanPackages).expect("Failed to rescan");

    // An Include folder appearing after the scan (e.g. headers generated during a CMake
    // configure) makes the index entry stale. `info` then re-scans the package and tries
    // to load its binary, which does not exist in a source-only checkout. Any resulting
    // diagnostics must not end up in stdout: consumers parse it as JSON.
    std::fs::create_dir_all(module_dir.join("Include")).expect("Failed to create Include dir");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nosman"))
        .arg("--workspace")
        .arg(&path)
        .args(["info", "test.sys.dummy", "1.0.0", "--relaxed"])
        .output()
        .expect("Failed to run nosman");

    assert!(
        out.status.success(),
        "info failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str::<serde_json::Value>(&stdout)
        .unwrap_or_else(|e| panic!("info stdout is not pure JSON ({}):\n{}", e, stdout));
}

#[test]
fn info_error_no_arguments() {
    let mut test = workspace!();

    let result = InfoCommand {}.run_get_info(&mut test.workspace, None, false, None);
    assert!(result.is_err(), "Should fail when no arguments are provided");
}
