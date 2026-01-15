# Nodos

Nodos is a highly extensible, node-graph based computing platform.

This repo is the workspace for Nodos development: engine SDKs, plugins/subsystems, and the toolchain used to generate projects.

## Toolchain
- `Toolchain/CMake`: workspace CMake entrypoint and helpers.
- `Toolchain/nosman`: package/module manager (builds `nodos.exe`).

## Develop Plugins/Subsystems (CMake)
Recommended workflow:
1. Put your plugin under `Module/` (or use `--plugin-dirs` for `dev gen` command).
2. Generate project files:
```
./nodos dev gen
```
Alternative:
```
cmake -S Toolchain/CMake -B Project -DCMAKE_BUILD_TYPE=RelWithDebInfo
```
3. Build and iterate in your IDE or with `./nodos dev build`.

### Nodos 1.4 and Later
#### Auto Target Generation
Nodos 1.4+ uses `.nosplugin` manifests to generate CMake targets for your plugins automatically.
- Put one `.nosplugin` in each plugin folder under `Module/`.
- The manifest declares the SDK version and dependencies; `nodos` will fetch the SDKs and packages as needed.
- No per-plugin CMake is required unless you need custom build steps.

#### Scanning rules
- The toolchain scans `Module/` (or `MODULE_DIRS`) recursively.
- If a folder contains a single `.nosplugin`, it becomes a plugin target.

#### Sharing common dependencies with NosPluginCommon.cmake
If you have several plugins under the same repository (or folder tree) and use common dependencies, add a `NosPluginCommon.cmake` file to that directory.
Define `nos_plugin_common(dir out_deps out_defs)` to return:
- `out_deps`: extra CMake targets or libraries to link.
- `out_defs`: preprocessor definitions to apply.
The toolchain calls this once per directory while scanning and applies the results to all plugins found under that subtree.

#### Extending a generated plugin target
If a plugin needs extra sources or build logic, add a `CMakeLists.txt` next to its `.nosplugin`.
After the target is created, the toolchain sets `NOS_PLUGIN_TARGET` and includes this file, so you can:
- add sources,
- add include paths,
- link extra libraries,
- set compile options/definitions.

#### Post-target hook
You can also define `nos_plugin_on_post_target_generated(target name)` (e.g., in `NosPluginCommon.cmake`) to run custom logic after a target is created.

#### Custom flatbuffers types
If your plugin defines its own `.fbs` files:
- Add a `Types/` folder, or list paths under `custom_types` in the `.nosplugin` manifest.
- The toolchain runs `flatc` and generates headers into `Include/<PluginName>`.
SDK-provided schemas live under `Types/` in the SDK.

### Legacy Workflow (Nodos 1.3 and Earlier)
Place engines/SDKs under `Engine/` (or set `ENGINE_FOLDER`) and use the legacy CMake workflow (per-plugin CMakeLists).

Sample `CMakeLists.txt`:
```cmake
# Select SDK
nos_find_sdk("1.3.0" NOS_PLUGIN_SDK_TARGET NOS_SUBSYSTEM_SDK_TARGET NOS_SDK_DIR)

# Pull dependency with nosman
nos_get_module("nos.sys.vulkan" "6.7" NOS_SYS_VULKAN_TARGET)

# Create plugin target
nos_add_plugin("MyPlugin" "${NOS_PLUGIN_SDK_TARGET};${NOS_SYS_VULKAN_TARGET}" "${CMAKE_CURRENT_SOURCE_DIR}/Include")

# Optional: generate flatbuffers
nos_generate_flatbuffers("Types" "${CMAKE_CURRENT_SOURCE_DIR}/Include/MyPlugin" "cpp" "${NOS_SDK_DIR}/Types" MyPlugin_generated)
target_sources(MyPlugin PUBLIC ${MyPlugin_generated})
```

You can also use the SDK without this repo by including the SDK's `cmake` folder in your project.

### Requirements
- CMake 3.24.2+
- C++20 compatible generator and compiler backends for CMake
- If you want to build Nodos Package Manager: Rust toolchain (cargo, rustc)

