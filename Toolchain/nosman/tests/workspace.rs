mod support;

use nosman::nosman::command::install::{InstallCommand, InstallFlags};
use nosman::nosman::workspace::AutoRescanResult;
use nosman::nosman::workspace::Workspace;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn auto_rescan_if_needed_workspace_not_ready() {
    let test_path = uninitialized_workspace!();

    let mut workspace = Workspace::from_root(&test_path);
    assert!(!workspace.ready(), "Workspace should not be ready initially");

    let result = workspace.auto_rescan_if_needed();
    assert!(result.is_ok(), "auto_rescan_if_needed should succeed when workspace is not ready");

    match result.unwrap() {
        AutoRescanResult::NoActionNeeded => {}
        other => panic!("Expected NoActionNeeded, got {:?}", other),
    }

    assert!(!workspace.ready(), "Workspace should still not be ready after auto_rescan_if_needed");
}

#[test]
fn auto_rescan_if_needed_no_index_file() {
    let mut test = workspace!();

    let index_path = test.workspace.get_nosman_index_filepath();
    if index_path.exists() {
        fs::remove_file(&index_path).expect("Failed to remove index file");
    }

    let result = test.workspace.auto_rescan_if_needed();
    assert!(result.is_ok(), "auto_rescan_if_needed should succeed when index file is missing");

    match result.unwrap() {
        AutoRescanResult::FullRescanMissingIndex => {}
        other => panic!("Expected FullRescanMissingIndex, got {:?}", other),
    }

    assert!(index_path.exists(), "Index file should be recreated after auto_rescan_if_needed");
}

#[test]
fn auto_rescan_if_needed_no_changes() {
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

    test.workspace.save().expect("Failed to save workspace");

    let result = test.workspace.auto_rescan_if_needed();
    assert!(result.is_ok(), "auto_rescan_if_needed should succeed when no changes");

    match result.unwrap() {
        AutoRescanResult::NoActionNeeded => {}
        other => panic!("Expected NoActionNeeded, got {:?}", other),
    }

    let versions = test.workspace.get_packages(package_name).unwrap();
    assert_eq!(versions.len(), 1, "Package should still be present after auto_rescan_if_needed");
}

#[test]
fn auto_rescan_if_needed_missing_manifest() {
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

    test.workspace.save().expect("Failed to save workspace");

    let mut manifest_path = None;
    for (_name, versions) in &test.workspace.packages {
        for package in versions.values().flatten() {
            let full_manifest_path = test.workspace.root.join(&package.manifest_path);
            if full_manifest_path.exists() {
                manifest_path = Some(full_manifest_path);
                break;
            }
        }
        if manifest_path.is_some() {
            break;
        }
    }

    if let Some(path) = manifest_path {
        fs::remove_file(&path).expect("Failed to remove manifest file");

        let result = test.workspace.auto_rescan_if_needed();
        assert!(result.is_ok(), "auto_rescan_if_needed should succeed when manifest is missing");

        match result.unwrap() {
            AutoRescanResult::FullRescanMissingManifests => {}
            other => panic!("Expected FullRescanMissingManifests, got {:?}", other),
        }
    }
}

#[test]
fn auto_rescan_if_needed_updated_manifest() {
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

    test.workspace.save().expect("Failed to save workspace");

    std::thread::sleep(Duration::from_millis(100));

    let mut manifest_path = None;
    for (_name, versions) in &test.workspace.packages {
        for package in versions.values().flatten() {
            let full_manifest_path = test.workspace.root.join(&package.manifest_path);
            if full_manifest_path.exists() {
                manifest_path = Some(full_manifest_path);
                break;
            }
        }
        if manifest_path.is_some() {
            break;
        }
    }

    if let Some(path) = manifest_path {
        let content = fs::read_to_string(&path).expect("Failed to read manifest file");
        fs::write(&path, content).expect("Failed to write manifest file");

        let result = test.workspace.auto_rescan_if_needed();
        assert!(result.is_ok(), "auto_rescan_if_needed should succeed when manifest is updated");

        match result.unwrap() {
            AutoRescanResult::PartialRescanUpdatedManifests(folders) => {
                assert!(!folders.is_empty(), "Should have at least one updated folder");
            }
            other => panic!("Expected PartialRescanUpdatedManifests, got {:?}", other),
        }
    }
}
