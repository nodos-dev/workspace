mod support;

use nosman::nosman::command::install::{InstallCommand, InstallFlags, InstallOp};
use nosman::nosman::index::SemVer;
use nosman::nosman::package::PackageIdentifier;
use std::path::PathBuf;

#[test]
fn install_no_deps() {
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
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    let versions = test.workspace.get_packages(package_name).unwrap();
    assert_eq!(versions.len(), 1);
}

#[test]
fn install_brings_dependencies() {
    let mut test = workspace!();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    test.workspace.fetch_package_releases(package_name);
    let index_entry = test
        .workspace
        .index_cache
        .get_package(package_name, version.as_str());
    assert!(index_entry.is_some());
    let index_entry = index_entry.unwrap().1;
    let mut requested_modules = index_entry
        .dependencies
        .clone()
        .expect("No dependencies found");
    requested_modules.push(PackageIdentifier {
        name: package_name.to_string(),
        version: version.clone(),
    });
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex,
        )
        .unwrap_or_else(|e| panic!("Failed to install {}: {:?}", package_name, e));
    assert_eq!(
        requested_modules.len(),
        test.workspace.get_local_package_count()
    );
    for requested in requested_modules {
        let installed = test.workspace.get_packages(requested.name.as_str()).unwrap();
        assert_eq!(installed.len(), 1);
        let installed = installed[0].info.clone();
        let requested_version = SemVer::parse_from_str(requested.version.as_str())
            .expect("Failed to parse semantic version");
        let installed_version = SemVer::parse_from_str(installed.id.version.as_str())
            .expect("Failed to parse semantic version");
        assert_eq!(installed.id.name, requested.name);
        assert!(installed_version.satisfies_requested_version(&requested_version));
    }
}

#[test]
fn install_skips_if_already_installed() {
    let mut test = workspace!();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    let op = InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    assert_eq!(op, InstallOp::Installed);

    let op_second = InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    assert_eq!(op_second, InstallOp::Skipped);
}

#[test]
fn install_with_only_major_version() {
    let mut test = workspace!();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6");
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    let versions = test.workspace.get_packages(package_name).unwrap();
    assert_eq!(versions.len(), 1);
}
