# Copyright MediaZ Teknoloji A.S. All Rights Reserved.
cmake_minimum_required(VERSION 3.24.2)

if (NOT DEFINED PROJECT_NAME)
	message(STATUS "PROJECT_NAME not defined. Defaulting to 'NodosWorkspace'.")
	set(PROJECT_NAME "NodosWorkspace")
endif()

project(${PROJECT_NAME} LANGUAGES)

set(WITH_NODOS_WORKSPACE ON)
if (NOT DEFINED NODOS_WORKSPACE_DIR)
	cmake_path(SET NODOS_WORKSPACE_CMAKE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/../..")
	# Convert to absolute path
	get_filename_component(NODOS_WORKSPACE_CMAKE_PATH "${NODOS_WORKSPACE_CMAKE_PATH}" ABSOLUTE)
	message(STATUS "NODOS_WORKSPACE_DIR not defined. Defaulting to ${NODOS_WORKSPACE_CMAKE_PATH}")
	set(NODOS_WORKSPACE_DIR "${NODOS_WORKSPACE_CMAKE_PATH}" CACHE PATH "Nodos workspace directory" FORCE)
endif()

if (NOT DEFINED NOSMAN_WORKSPACE_DIR)
	set(NOSMAN_WORKSPACE_DIR "${NODOS_WORKSPACE_DIR}" CACHE PATH "nosman workspace directory" FORCE)
endif()

set(NOS_VERSIONS "")
set(NOS_SDK_DIRS "")

set(NOS_ENGINE_FOLDER ${NODOS_WORKSPACE_DIR}/Engine)
file(GLOB NOS_ENGINE_DIRS "${NOS_ENGINE_FOLDER}/*")

set(NOS_ENGINE_DIR_INDEX 0)
foreach(nos_engine_dir ${NOS_ENGINE_DIRS})
	if(IS_DIRECTORY ${nos_engine_dir})
		# If engine directory does not contain a CMakeLists.txt, include SDK/cmake/VersionInfo.cmake
		if(NOT EXISTS "${nos_engine_dir}/CMakeLists.txt")
			message(STATUS "Including ${nos_engine_dir}/SDK/cmake/VersionInfo.cmake")
			include(${nos_engine_dir}/SDK/cmake/VersionInfo.cmake)
		else()
			add_subdirectory(${nos_engine_dir} "${CMAKE_CURRENT_BINARY_DIR}/EngineDir${NOS_ENGINE_DIR_INDEX}")
			math(EXPR NOS_ENGINE_DIR_INDEX "${NOS_ENGINE_DIR_INDEX} + 1")
		endif()
	endif()
endforeach()

# Prepare targets
list(LENGTH NOS_VERSIONS NOS_SDK_COUNT)
math(EXPR NOS_SDK_END_RANGE "${NOS_SDK_COUNT} - 1")
message(STATUS "Found ${NOS_SDK_END_RANGE} Nodos SDKs")
 
foreach(val RANGE 1 ${NOS_SDK_END_RANGE})
	list(GET NOS_VERSIONS ${val} nos_version)
	list(GET NOS_SDK_DIRS ${val} nos_sdk_dir)

	message(STATUS "(${val}/${NOS_SDK_END_RANGE}) Creating targets for Nodos version ${nos_version} at ${nos_sdk_dir}")

	# Convert version 1.2.3 to 1_2_3
	string(REPLACE "." "_" version_target_suffix "${nos_version}")

	# Create nosPluginSDK target
	if(TARGET nosPluginSDK_${version_target_suffix})
		message(FATAL_ERROR "Multiple Nodos SDKs with the same version found under Engine folder. This is not supported.")
	endif()

	add_library(nosPluginSDK_${version_target_suffix} INTERFACE)
	target_compile_definitions(nosPluginSDK_${version_target_suffix} INTERFACE "-DNOMINMAX")
	target_include_directories(nosPluginSDK_${version_target_suffix} INTERFACE ${nos_sdk_dir}/include)

	# Create nosSubsystemSDK target
	add_library(nosSubsystemSDK_${version_target_suffix} INTERFACE)
	target_compile_definitions(nosSubsystemSDK_${version_target_suffix} INTERFACE "-DNOMINMAX")
	target_include_directories(nosSubsystemSDK_${version_target_suffix} INTERFACE ${nos_sdk_dir}/include)

	# Exclude from all
	set_target_properties(nosPluginSDK_${version_target_suffix} PROPERTIES EXCLUDE_FROM_ALL TRUE)
	set_target_properties(nosSubsystemSDK_${version_target_suffix} PROPERTIES EXCLUDE_FROM_ALL TRUE)
endforeach()

# Find nosman
if (NOT DEFINED NOSMAN_EXECUTABLE)
	if (EXISTS "${NODOS_WORKSPACE_DIR}/nosman${CMAKE_EXECUTABLE_SUFFIX}")
		set(NOSMAN_EXECUTABLE "${NODOS_WORKSPACE_DIR}/nosman${CMAKE_EXECUTABLE_SUFFIX}")
	endif()
endif()

# Check if file exists
if (NOT EXISTS "${NOSMAN_EXECUTABLE}")
	message(STATUS "nosman not found at ${NOSMAN_EXECUTABLE}. Building...")
	execute_process(
		COMMAND cargo build --release --quiet
		WORKING_DIRECTORY "${NODOS_WORKSPACE_DIR}/Toolchain/nosman"
		RESULT_VARIABLE NOSMAN_RESULT
	)
	if (NOSMAN_RESULT EQUAL 0)
		# Copy the executable to the root
		configure_file("${NODOS_WORKSPACE_DIR}/Toolchain/nosman/target/release/nosman${CMAKE_EXECUTABLE_SUFFIX}" "${NODOS_WORKSPACE_DIR}/nosman${CMAKE_EXECUTABLE_SUFFIX}" COPYONLY)
		set(NOSMAN_EXECUTABLE "${NODOS_WORKSPACE_DIR}/nosman")
		message(STATUS "nosman built successfully at ${NOSMAN_EXECUTABLE}")
	else()
		message(FATAL_ERROR "Failed to build nosman")
		return()
	endif()
else()
	message(STATUS "Found nosman at ${NOSMAN_EXECUTABLE}")
endif()

if (NOT EXISTS "${NOSMAN_WORKSPACE_DIR}/.nosman")
	message(STATUS "Initializing workspace at ${NOSMAN_WORKSPACE_DIR}")
	execute_process(
		COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" init
		RESULT_VARIABLE NOSMAN_RESULT
	)
endif()

# Includes
include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/FindNodosSDK.cmake)
include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/GetModule.cmake)
include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/Projects.cmake)

if(NOT DEFINED MODULE_DIRS OR MODULE_DIRS STREQUAL "")
	set(MODULE_DIRS "${NODOS_WORKSPACE_DIR}/Module" CACHE INTERNAL "Module directories" FORCE)
endif()

function(collect_first_cmake_directories dir out_dirs)
	file(GLOB SUBDIRS RELATIVE ${dir} ${dir}/*)

	foreach(subdir ${SUBDIRS})
		if(IS_DIRECTORY ${dir}/${subdir})
			if(EXISTS "${dir}/${subdir}/CMakeLists.txt")
				set(${out_dirs} ${${out_dirs}} ${dir}/${subdir} CACHE INTERNAL "Module directories" FORCE)
				message("Found module directory: ${dir}/${subdir}")
			else()
				collect_first_cmake_directories(${dir}/${subdir} ${out_dirs})
			endif()
		endif()
	endforeach()
endfunction()

set(NOS_MODULE_DIR_INDEX 0)
set(ALL_MODULE_DIRS "" CACHE INTERNAL "All module directories" FORCE)
foreach(cur_module_dir ${MODULE_DIRS})
	# If relative, should be relative to NODOS_WORKSPACE_DIR
	if(NOT IS_ABSOLUTE ${cur_module_dir})
		set(cur_module_dir "${NODOS_WORKSPACE_DIR}/${cur_module_dir}")
	endif()
	message(STATUS "Scanning for modules in ${cur_module_dir}")
	
	collect_first_cmake_directories("${cur_module_dir}" ALL_MODULE_DIRS)
	list(LENGTH ALL_MODULE_DIRS all_dirs_count)
	message(STATUS "Found ${all_dirs_count} module paths under ${cur_module_dir}: ${ALL_MODULE_DIRS}")

	foreach(dir ${ALL_MODULE_DIRS})
		if(IS_DIRECTORY ${dir})
			message("Processing module directory: ${dir}")
			add_subdirectory(${dir} "${CMAKE_CURRENT_BINARY_DIR}/ModuleDir${NOS_MODULE_DIR_INDEX}")
			math(EXPR NOS_MODULE_DIR_INDEX "${NOS_MODULE_DIR_INDEX} + 1")
		endif()
	endforeach()
endforeach()