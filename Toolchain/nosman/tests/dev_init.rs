mod support;

use nosman::nosman::command::dev::DevInitCommand;
use std::fs;
use support::WorkspaceGen;

#[test]
fn dev_init_cmake_copies_toolchain() {
    let test = WorkspaceGen::new_random();

    DevInitCommand {}
        .run_init(&test.workspace, "cmake")
        .expect("dev init cmake failed");

    let cmake_root = test.workspace.root.join("Toolchain").join("CMake");
    assert!(cmake_root.join("CMakeLists.txt").exists());
    assert!(cmake_root.join("Scripts").join("Projects.cmake").exists());
}

#[test]
fn dev_init_cmake_overwrites_when_exists() {
    let test = WorkspaceGen::new_random();

    DevInitCommand {}
        .run_init(&test.workspace, "cmake")
        .expect("dev init cmake failed");

    let cmake_root = test.workspace.root.join("Toolchain").join("CMake");
    let cmake_list_path = cmake_root.join("CMakeLists.txt");
    fs::write(&cmake_list_path, "modified").expect("Failed to modify CMakeLists.txt");

    DevInitCommand {}
        .run_init(&test.workspace, "cmake")
        .expect("dev init cmake reinit failed");

    let expected = fs::read_to_string("../CMake/CMakeLists.txt")
        .expect("Failed to read expected CMakeLists.txt");
    let actual = fs::read_to_string(&cmake_list_path)
        .expect("Failed to read CMakeLists.txt after reinit");
    assert_eq!(actual, expected);
}
