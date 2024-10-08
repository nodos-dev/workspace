set(USE_NOS_SDK_VERSION "<CMAKE_LATEST_NOS_VERSION>")
nos_find_sdk(${USE_NOS_SDK_VERSION} NOS_PLUGIN_SDK_TARGET NOS_SUBSYSTEM_SDK_TARGET NOS_SDK_DIR)
if (NOT DEFINED NOS_SDK_DIR)
    message(FATAL_ERROR "Nodos SDK with version ${USE_NOS_SDK_VERSION} not found, please either install it or choose different version")
endif()

set(MODULE_DEPENDENCIES <CMAKE_MODULE_DEPENDENCIES>)
set(dep_idx 0)
foreach(module_name_version ${MODULE_DEPENDENCIES})
    # module_name_version: <MODULE_NAME>-<MODULE_VERSION>
    string(REPLACE "-" ";" module_name_version ${module_name_version})
    list(GET module_name_version 0 module_name)
    list(GET module_name_version 1 module_version)
    nos_get_module("${module_name}" "${module_version}" DEP_${dep_idx})
    list(APPEND MODULE_DEPENDENCIES_TARGETS ${DEP_${dep_idx}})
endforeach()
list(APPEND MODULE_DEPENDENCIES_TARGETS ${NOS_SUBSYSTEM_SDK_TARGET})

list(APPEND INCLUDE_FOLDERS ${CMAKE_CURRENT_SOURCE_DIR} "${CMAKE_CURRENT_SOURCE_DIR}/Include")
nos_add_subsystem("<CMAKE_PROJECT_NAME>" "${MODULE_DEPENDENCIES_TARGETS}" "${INCLUDE_FOLDERS}")

#Helpers need C++20
set_target_properties("<CMAKE_PROJECT_NAME>" PROPERTIES CXX_STANDARD 20)