# Copyright MediaZ Teknoloji A.S. All Rights Reserved.
cmake_minimum_required(VERSION 3.24.2)

if (NOT DEFINED PROJECT_NAME)
	message(STATUS "PROJECT_NAME not defined. Defaulting to 'NodosWorkspace'.")
	set(PROJECT_NAME "NodosWorkspace")
endif()

project(${PROJECT_NAME})

set(CMAKE_POSITION_INDEPENDENT_CODE ON)

if(CMAKE_CXX_COMPILER_ID STREQUAL "GNU")
	set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -std=c++23 -Wno-changes-meaning -fpermissive -fPIC")
endif()
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
else()
	# Convert to absolute path
	get_filename_component(NOSMAN_WORKSPACE_DIR "${NOSMAN_WORKSPACE_DIR}" ABSOLUTE)
	set(NOSMAN_WORKSPACE_DIR "${NOSMAN_WORKSPACE_DIR}" CACHE PATH "nosman workspace directory" FORCE)
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
 
if (NOS_SDK_END_RANGE GREATER_EQUAL 1)
	message(STATUS "Found ${NOS_SDK_END_RANGE} Nodos SDKs")
	foreach(val RANGE 1 ${NOS_SDK_END_RANGE})
		list(GET NOS_VERSIONS ${val} nos_version)
		list(GET NOS_SDK_DIRS ${val} nos_sdk_dir)
	
		message(STATUS "(${val}/${NOS_SDK_END_RANGE}) Creating targets for Nodos version ${nos_version} at ${nos_sdk_dir}")
	
		# Convert version 1.2.3 to 1_2_3
		string(REPLACE "." "_" version_target_suffix "${nos_version}")
	
		set(nos_plugin_sdk_target nosPluginSDK_${version_target_suffix})
		set(nos_subsystem_sdk_target nosSubsystemSDK_${version_target_suffix})
		# Create nosPluginSDK target
		if(TARGET ${nos_plugin_sdk_target})
			message(FATAL_ERROR "Multiple Nodos SDKs with the same version found under Engine folder. This is not supported.")
		endif()
	
		add_library(${nos_plugin_sdk_target} INTERFACE)
		target_compile_definitions(${nos_plugin_sdk_target} INTERFACE "-DNOMINMAX")
		target_include_directories(${nos_plugin_sdk_target} INTERFACE ${nos_sdk_dir}/include)
	
		# Create nosSubsystemSDK target
		add_library(${nos_subsystem_sdk_target} INTERFACE)
		target_compile_definitions(${nos_subsystem_sdk_target} INTERFACE "-DNOMINMAX")
		target_include_directories(${nos_subsystem_sdk_target} INTERFACE ${nos_sdk_dir}/include)
	
		# Exclude from all
		set_target_properties(${nos_plugin_sdk_target} PROPERTIES EXCLUDE_FROM_ALL TRUE)
		set_target_properties(${nos_subsystem_sdk_target} PROPERTIES EXCLUDE_FROM_ALL TRUE)
	
		# If binaries folder exists, add a custom target for nosLauncher and nosEditor and VS_DEBUGGER options
		set(nos_binaries_dir "${nos_sdk_dir}/../Binaries")
		if(EXISTS "${nos_binaries_dir}")
			# Create nosLauncher target
			set(nos_launcher_target nosLauncher_${version_target_suffix})
			add_custom_target(${nos_launcher_target}
				COMMAND ${nos_binaries_dir}/nosLauncher
				WORKING_DIRECTORY ${nos_sdk_dir}/bin
				DEPENDS ${nos_plugin_sdk_target} ${nos_subsystem_sdk_target}
			)
	
			# Create nosEditor target
			set(nos_editor_target nosEditor_${version_target_suffix})
			add_custom_target(${nos_editor_target}
				COMMAND ${nos_binaries_dir}/nosEditor
				WORKING_DIRECTORY ${nos_sdk_dir}/bin
				DEPENDS ${nos_plugin_sdk_target} ${nos_subsystem_sdk_target}
			)
	
			set_target_properties(${nos_launcher_target} PROPERTIES VS_DEBUGGER_WORKING_DIRECTORY ${nos_binaries_dir})
			set_target_properties(${nos_launcher_target} PROPERTIES VS_DEBUGGER_COMMAND ${nos_binaries_dir}/nosLauncher)
			set_target_properties(${nos_editor_target} PROPERTIES VS_DEBUGGER_WORKING_DIRECTORY ${nos_binaries_dir})
			set_target_properties(${nos_editor_target} PROPERTIES VS_DEBUGGER_COMMAND ${nos_binaries_dir}/nosEditor)
			set_target_properties(${nos_launcher_target} PROPERTIES FOLDER "Nodos")
			set_target_properties(${nos_editor_target} PROPERTIES FOLDER "Nodos")
		endif()
	endforeach()
endif()

# Includes
include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/Utils.cmake)
include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/FindNodosSDK.cmake)
include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/Projects.cmake)

# If Toolchain/nosman/src exists, setup cargo build command that depends on .rs files to build and copy the executable to the root
if ((NOT NOS_INVOKED_FROM_NOSMAN) AND (EXISTS "${NODOS_WORKSPACE_DIR}/Toolchain/nosman/src"))
	set(NOSMAN_BUILD_DIR "${NODOS_WORKSPACE_DIR}/Toolchain/nosman/target")

	message(STATUS "Found nosman sources. Checking/building nosman...")
	execute_process(
		COMMAND cargo build --release --quiet
		WORKING_DIRECTORY "${NODOS_WORKSPACE_DIR}/Toolchain/nosman"
		RESULT_VARIABLE NOSMAN_RESULT
	)
	if (NOSMAN_RESULT EQUAL 0)
		# Copy the executable to the root
		file(COPY "${NOSMAN_BUILD_DIR}/release/nosman${CMAKE_EXECUTABLE_SUFFIX}" DESTINATION "${NODOS_WORKSPACE_DIR}")
		file(RENAME "${NODOS_WORKSPACE_DIR}/nosman${CMAKE_EXECUTABLE_SUFFIX}" "${NODOS_WORKSPACE_DIR}/nodos${CMAKE_EXECUTABLE_SUFFIX}")
	else()
		message(FATAL_ERROR "Failed to build nosman")
		return()
	endif()
endif()

# Find nosman
if (NOT DEFINED NOSMAN_EXECUTABLE)
	if (EXISTS "${NODOS_WORKSPACE_DIR}/nodos${CMAKE_EXECUTABLE_SUFFIX}")
		set(NOSMAN_EXECUTABLE "${NODOS_WORKSPACE_DIR}/nodos${CMAKE_EXECUTABLE_SUFFIX}")
	elseif(EXISTS "${NODOS_WORKSPACE_DIR}/nosman${CMAKE_EXECUTABLE_SUFFIX}")
		set(NOSMAN_EXECUTABLE "${NODOS_WORKSPACE_DIR}/nosman${CMAKE_EXECUTABLE_SUFFIX}")
	endif()
endif()
if (NOT EXISTS "${NOSMAN_EXECUTABLE}")
	message(FATAL_ERROR "nosman executable not found. Please set NOSMAN_EXECUTABLE to the path of the nosman executable.")
else()
	message(STATUS "Found nosman at ${NOSMAN_EXECUTABLE}")
endif()

if (NOT EXISTS "${NOSMAN_WORKSPACE_DIR}/.nosman/index")
	message(STATUS "Initializing workspace at ${NOSMAN_WORKSPACE_DIR}")
	execute_process(
		COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" init
		RESULT_VARIABLE NOSMAN_RESULT
	)
endif()

if(NOT DEFINED MODULE_DIRS OR MODULE_DIRS STREQUAL "")
	set(MODULE_DIRS "${NODOS_WORKSPACE_DIR}/Module" CACHE INTERNAL "Module directories" FORCE)
endif()

function(collect_first_cmake_directories dir out_dirs)
	file(GLOB SUBDIRS RELATIVE ${dir} ${dir}/*)

	if(EXISTS "${dir}/CMakeLists.txt")
		set(${out_dirs} ${${out_dirs}} ${dir} CACHE INTERNAL "Module directories" FORCE)
		message("Found module directory: ${dir}/${subdir}")
	else()
		foreach(subdir ${SUBDIRS})
			if(IS_DIRECTORY ${dir}/${subdir})
				collect_first_cmake_directories(${dir}/${subdir} ${out_dirs})
			endif()
		endforeach()
	endif()
endfunction()

set(NOS_MODULE_DIR_INDEX 0)
set(ALL_MODULE_DIRS "" CACHE INTERNAL "All module directories" FORCE)
foreach(cur_module_dir ${MODULE_DIRS})
	# If relative, should be relative to NODOS_WORKSPACE_DIR
	if(NOT IS_ABSOLUTE ${cur_module_dir})
		set(cur_module_dir "${NODOS_WORKSPACE_DIR}/${cur_module_dir}")
	endif()
	nos_colored_message(COLOR GREEN "Scanning for modules in ${cur_module_dir}")
	collect_first_cmake_directories("${cur_module_dir}" ALL_MODULE_DIRS)
endforeach()

foreach(dir ${ALL_MODULE_DIRS})
	if(IS_DIRECTORY ${dir})
		nos_colored_message(COLOR GREEN "Processing module directory: ${dir}")
		add_subdirectory(${dir} "${CMAKE_CURRENT_BINARY_DIR}/ModuleDir${NOS_MODULE_DIR_INDEX}")
		math(EXPR NOS_MODULE_DIR_INDEX "${NOS_MODULE_DIR_INDEX} + 1")
	endif()
endforeach()