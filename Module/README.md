# Nodos Module Development
Bring your module repositories here, with `git clone` or other methods.

## Usage
Find the Nodos SDK targets using:
```cmake
nos_find_sdk("1.2.0" NOS_PLUGIN_SDK_TARGET NOS_SUBSYSTEM_SDK_TARGET NOS_SDK_DIR)
```

Then you can link your module with the targets:
```cmake
target_link_libraries(${PROJECT_NAME} PRIVATE ${NOS_PLUGIN_SDK_TARGET})
```

Nodos uses flatbuffers as data serialization/schema language.
Use flatc in [mediaz/Tools](https://github.com/mediaz/Tools) to generate code.
Builtin flatbuffers are also available in the SDK under `/types` folder.

You can still use Nodos SDK without this repository, by including SDK/cmake folder in your project:
```cmake
list(APPEND CMAKE_MODULE_PATH "${NODOS_SDK_DIR}/cmake")
find_package(nosPluginSDK REQUIRED)
find_package(nosSubsystemSDK REQUIRED)
```