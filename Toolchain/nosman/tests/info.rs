mod support;

use nosman::nosman::command::info::InfoCommand;
use nosman::nosman::command::install::{InstallCommand, InstallFlags};
use std::path::PathBuf;
use support::WorkspaceGen;

#[test]
fn info_by_package_and_version() {
    let mut test = WorkspaceGen::new_random();
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
    let mut test = WorkspaceGen::new_random();
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
    let mut test = WorkspaceGen::new_random();
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
fn info_error_no_arguments() {
    let mut test = WorkspaceGen::new_random();

    let result = InfoCommand {}.run_get_info(&mut test.workspace, None, false, None);
    assert!(result.is_err(), "Should fail when no arguments are provided");
}
