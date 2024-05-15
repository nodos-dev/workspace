# Nodos Workspace

## Developing a plugin/subsystem for Nodos

### With CMake
1. Download the engines (or SDK only distributions) under `Engine/` folder.
2. Place your module repo under `Module/` folder.
3. Add your module to the `CMakeLists.txt` file in the root of the project.
A sample CMakeLists.txt file:
```cmake
# Specify which versions you want to use
nos_find_sdk("1.2.0" NOS_PLUGIN_SDK_TARGET NOS_SUBSYSTEM_SDK_TARGET NOS_SDK_DIR)

# If you have a module or subsystem dependency, nosman will get the dependency for you:
nos_get_module("nos.sys.vulkan" "5.2" NOS_SYS_VULKAN_TARGET)

# Under a folder where you have your plugin (with .noscfg)
nos_add_plugin("MyPlugin" "${NOS_PLUGIN_SDK_TARGET};${NOS_SYS_VULKAN_TARGET};${OTHER_DEPENDENCIES}" "${INCLUDE_FOLDERS}")

# You can generate flatbuffers files with:
nos_generate_flatbuffers("${YOUR_FBS_FOLDERS}" "${DESTINATION_FOLDER}" "cpp" "${FBS_INCLUDE_FOLDERS}" MY_PLUGIN_GENERATED_FILES)
target_sources(MyPlugin PUBLIC ${MY_PLUGIN_GENERATED_FILES})
```
4. Run `cmake` to generate the project files: `cmake -S ./Toolchain/CMake -B Project -DPROJECT_NAME=<your project name>`

Nodos uses flatbuffers as data serialization/schema language.
Use flatc in [mediaz/Tools](https://github.com/mediaz/Tools) to generate code.
Builtin flatbuffers are also available in the SDK under `/types` folder.

You can still use Nodos SDK without this repository, by including SDK/cmake folder in your project:
```cmake
list(APPEND CMAKE_MODULE_PATH "${NODOS_SDK_DIR}/cmake")
find_package(nosPluginSDK REQUIRED)
find_package(nosSubsystemSDK REQUIRED)
```