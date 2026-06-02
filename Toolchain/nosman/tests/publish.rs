mod support;

use nosman::nosman::command::publish::PublishCommand;
use nosman::nosman::workspace::Workspace;
use std::fs;
use std::path::Path;

fn write_plugin_manifest(pkg_dir: &Path, name: &str, version: &str, sdk_version: Option<&str>) {
    fs::create_dir_all(pkg_dir).expect("Failed to create package dir");
    let mut manifest = serde_json::json!({
        "info": {
            "id": { "name": name, "version": version },
            "description": "Test plugin for publish tests",
        },
        "binary_path": "Binaries/Foo",
    });
    if let Some(sv) = sdk_version {
        manifest["sdk_version"] = serde_json::Value::String(sv.to_string());
    }
    let manifest_path = pkg_dir.join(format!("{}.noscfg", name));
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap())
        .expect("Failed to write manifest");
}

#[test]
fn publish_dry_run_without_workspace_for_modern_plugin() {
    let dir = uninitialized_workspace!();
    let pkg_dir = dir.join("pkg");
    write_plugin_manifest(&pkg_dir, "test.plugin.modern", "1.0.0", Some("1.4.0"));

    let mut ws = Workspace::from_root(&dir);
    assert!(!ws.ready(), "expected an uninitialized workspace");

    PublishCommand {}
        .publish(
            &mut ws, true, false, &pkg_dir,
            None, None, "", None,
            &vec![], None,
            nodos_store_client::PackageVisibility::Public,
            None,
            false, false, false,
        )
        .expect("dry-run publish should succeed outside a workspace for a 1.4+ plugin");
}

#[test]
fn publish_legacy_plugin_without_workspace_errors() {
    let dir = uninitialized_workspace!();
    let pkg_dir = dir.join("pkg");
    write_plugin_manifest(&pkg_dir, "test.plugin.legacy", "1.0.0", None);

    let mut ws = Workspace::from_root(&dir);
    assert!(!ws.ready(), "expected an uninitialized workspace");

    let res = PublishCommand {}.publish(
        &mut ws, true, false, &pkg_dir,
        None, None, "", None,
        &vec![], None,
        nodos_store_client::PackageVisibility::Public,
        None,
        false, false, false,
    );
    let err = res.expect_err("legacy plugin publish without workspace should fail");
    let msg = format!("{}", err);
    assert!(
        msg.contains("sdk_version") && msg.contains("workspace"),
        "unexpected error message: {}",
        msg
    );
}
