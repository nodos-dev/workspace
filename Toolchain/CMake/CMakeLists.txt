# Copyright MediaZ Teknoloji A.S. All Rights Reserved.
cmake_minimum_required(VERSION 3.24.2)

if (NOT DEFINED PROJECT_NAME)
	message(STATUS "PROJECT_NAME not defined. Defaulting to 'NodosWorkspace'.")
	set(PROJECT_NAME "NodosWorkspace")
endif()

project(${PROJECT_NAME})

# Includes
include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/Utils.cmake)

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
set(NOS_PLUGIN_SDK_VERSIONS "")
set(NOS_SUBSYSTEM_SDK_VERSIONS "")
set(NOS_SDK_DIRS "")

macro(_nos_try_include_sdk_file engine_folder relative_path)
	set(cmake_upper_path "${engine_folder}/SDK/CMake/${relative_path}")
	set(cmake_lower_path "${engine_folder}/SDK/cmake/${relative_path}")
	
	if(EXISTS "${cmake_upper_path}")
		message(STATUS "Including ${cmake_upper_path}")
		include("${cmake_upper_path}")
	elseif(EXISTS "${cmake_lower_path}")
		message(STATUS "Including ${cmake_lower_path}")
		include("${cmake_lower_path}")
	endif()
endmacro()

macro(_nos_process_engine_folder engine_folder binary_dir_index)
	if(IS_DIRECTORY ${engine_folder})
		# If engine directory does not contain a CMakeLists.txt, include SDK/CMake/VersionInfo.cmake.
		if(NOT EXISTS "${engine_folder}/CMakeLists.txt")
			_nos_try_include_sdk_file("${engine_folder}" "VersionInfo.cmake")
		else()
			add_subdirectory(${engine_folder} "${CMAKE_CURRENT_BINARY_DIR}/EngineDir${binary_dir_index}")
		endif()

		list(LENGTH NOS_SDK_DIRS nos_sdk_count)
		math(EXPR nos_sdk_end_range "${nos_sdk_count} - 1")
		list(GET NOS_SDK_DIRS ${nos_sdk_end_range} cur_nos_sdk)

		file(READ "${cur_nos_sdk}/info.json" nos_info_json)
		string(JSON nos_plugin_sdk_version GET "${nos_info_json}" "plugin_sdk_version")
		string(JSON nos_subsystem_sdk_version ERROR_VARIABLE err GET "${nos_info_json}" "subsystem_sdk_version")
		if (err STREQUAL "NOTFOUND")
			set(nos_subsystem_sdk_version ${nos_plugin_sdk_version})
		endif()

		set(NOS_PLUGIN_SDK_VERSIONS "${NOS_PLUGIN_SDK_VERSIONS};${nos_plugin_sdk_version}")
		set(NOS_SUBSYSTEM_SDK_VERSIONS "${NOS_SUBSYSTEM_SDK_VERSIONS};${nos_subsystem_sdk_version}")
	else()
		nos_fatal_error("ENGINE_FOLDER is not a valid directory.")
	endif()
endmacro()

if(NOT DEFINED ENGINE_FOLDER OR ENGINE_FOLDER STREQUAL "")
	nos_colored_message(COLOR YELLOW "ENGINE_FOLDER not defined. Project will include Nodos engines in ${NODOS_WORKSPACE_DIR}/Engine")

	file(GLOB NOS_ENGINE_DIRS "${NODOS_WORKSPACE_DIR}/Engine/*")

	set(NOS_ENGINE_DIR_INDEX 0)
	foreach(nos_engine_dir ${NOS_ENGINE_DIRS})
		if(IS_DIRECTORY ${nos_engine_dir})
			_nos_process_engine_folder(${nos_engine_dir} ${NOS_ENGINE_DIR_INDEX})
			math(EXPR NOS_ENGINE_DIR_INDEX "${NOS_ENGINE_DIR_INDEX} + 1")
		endif()
	endforeach()
else()
	nos_colored_message(COLOR GREEN "Using Nodos SDK in ${ENGINE_FOLDER}...")
	set(ENGINE_FOLDER ${NODOS_WORKSPACE_DIR}/${ENGINE_FOLDER})
	_nos_process_engine_folder(${ENGINE_FOLDER} 0)
endif()

# Prepare targets
list(LENGTH NOS_VERSIONS NOS_SDK_COUNT)
math(EXPR NOS_SDK_END_RANGE "${NOS_SDK_COUNT} - 1")
 
if (NOS_SDK_END_RANGE GREATER_EQUAL 1)
	message(STATUS "Found ${NOS_SDK_END_RANGE} Nodos SDKs")
	foreach(val RANGE 1 ${NOS_SDK_END_RANGE})
		list(GET NOS_VERSIONS ${val} nos_version)
		list(GET NOS_SDK_DIRS ${val} nos_sdk_dir)
		list(GET NOS_PLUGIN_SDK_VERSIONS ${val} nos_plugin_sdk_version)
		list(GET NOS_SUBSYSTEM_SDK_VERSIONS ${val} nos_subsystem_sdk_version)

		# Convert version 1.2.3 to 1_2_3
		string(REPLACE "." "_" version_target_suffix "${nos_version}")

		if (nos_version VERSION_GREATER_EQUAL "1.4.0")
			set(sdk_root ${nos_sdk_dir}/Plugin)
			set(sdk_include_dir ${nos_sdk_dir}/Plugin/Include)
		else()
			set(sdk_root ${nos_sdk_dir})
			set(sdk_include_dir ${nos_sdk_dir}/include)
		endif()
	
		if (NOT nos_subsystem_sdk_version)
			message(STATUS "(${val}/${NOS_SDK_END_RANGE}) Creating targets for Nodos version ${nos_version} (Plugin SDK: ${nos_plugin_sdk_version}) at ${nos_sdk_dir}")
		else()
			message(STATUS "(${val}/${NOS_SDK_END_RANGE}) Creating targets for Nodos version ${nos_version} (Plugin SDK: ${nos_plugin_sdk_version}, Subsystem SDK: ${nos_subsystem_sdk_version}) at ${nos_sdk_dir}")
		endif()
		string(REPLACE "." "_" plugin_sdk_version_target_suffix "${nos_plugin_sdk_version}")

		# nosPluginSDK target
		# -------------------
		set(nos_plugin_sdk_target nosPluginSDK_${plugin_sdk_version_target_suffix})

		if(TARGET ${nos_plugin_sdk_target})
			nos_fatal_error("Multiple Nodos Plugin SDKs with the same version found under Engine folder. This is not supported.")
		endif()
		
		# Create targets
		add_library(${nos_plugin_sdk_target} INTERFACE)
		target_compile_definitions(${nos_plugin_sdk_target} INTERFACE "-DNOMINMAX")
		target_include_directories(${nos_plugin_sdk_target} INTERFACE ${sdk_include_dir})

		# Exclude from all
		set_target_properties(${nos_plugin_sdk_target} PROPERTIES EXCLUDE_FROM_ALL TRUE)
		
		# Add natvis
		set(natvis_file "${sdk_root}/nosSDKCommon.natvis")
		if(EXISTS ${natvis_file})
			target_sources(${nos_plugin_sdk_target} INTERFACE ${natvis_file})
		endif()

		# -------------------

		if (nos_subsystem_sdk_version)
			string(REPLACE "." "_" subsystem_sdk_target_suffix "${nos_subsystem_sdk_version}")
			set(nos_subsystem_sdk_target nosSubsystemSDK_${subsystem_sdk_target_suffix})

			# Create nosSubsystemSDK target
			add_library(${nos_subsystem_sdk_target} INTERFACE)
			target_compile_definitions(${nos_subsystem_sdk_target} INTERFACE "-DNOMINMAX")
			target_include_directories(${nos_subsystem_sdk_target} INTERFACE ${sdk_include_dir})
			set_target_properties(${nos_subsystem_sdk_target} PROPERTIES EXCLUDE_FROM_ALL TRUE)
			if (EXISTS ${natvis_file})
				target_sources(${nos_subsystem_sdk_target} INTERFACE ${natvis_file})
			endif()
		endif()
	
		# If binaries folder exists, add a custom target for nosLauncher and nosEditor and VS_DEBUGGER options
		set(nos_binaries_dir "${nos_sdk_dir}/../Binaries")
		if(EXISTS "${nos_binaries_dir}")
			# Create nosLauncher target
			set(nos_launcher_target nosLauncher_${version_target_suffix})
			add_custom_target(${nos_launcher_target}
				COMMAND ${nos_binaries_dir}/nosLauncher${CMAKE_EXECUTABLE_SUFFIX}
				WORKING_DIRECTORY ${nos_binaries_dir}
				DEPENDS ${nos_plugin_sdk_target} ${nos_subsystem_sdk_target}
			)
	
			# Create nosEditor target
			set(nos_editor_target nosEditor_${version_target_suffix})
			add_custom_target(${nos_editor_target}
				COMMAND ${nos_binaries_dir}/nosEditor${CMAKE_EXECUTABLE_SUFFIX}
				WORKING_DIRECTORY ${nos_binaries_dir}
				DEPENDS ${nos_plugin_sdk_target} ${nos_subsystem_sdk_target}
			)
	
			set_target_properties(${nos_launcher_target} PROPERTIES VS_DEBUGGER_WORKING_DIRECTORY ${nos_binaries_dir})
			set_target_properties(${nos_launcher_target} PROPERTIES VS_DEBUGGER_COMMAND ${nos_binaries_dir}/nosLauncher${CMAKE_EXECUTABLE_SUFFIX})
			set_target_properties(${nos_editor_target} PROPERTIES VS_DEBUGGER_WORKING_DIRECTORY ${nos_binaries_dir})
			set_target_properties(${nos_editor_target} PROPERTIES VS_DEBUGGER_COMMAND ${nos_binaries_dir}/nosEditor${CMAKE_EXECUTABLE_SUFFIX})
			set_target_properties(${nos_launcher_target} PROPERTIES FOLDER "Nodos")
			set_target_properties(${nos_editor_target} PROPERTIES FOLDER "Nodos")
		endif()
	endforeach()
endif()

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
		nos_fatal_error("Failed to build nosman")
		return()
	endif()
endif()

unset(NOS_INVOKED_FROM_NOSMAN CACHE)

# Find nosman
if (NOT DEFINED NOSMAN_EXECUTABLE)
	if (EXISTS "${NODOS_WORKSPACE_DIR}/nodos${CMAKE_EXECUTABLE_SUFFIX}")
		set(NOSMAN_EXECUTABLE "${NODOS_WORKSPACE_DIR}/nodos${CMAKE_EXECUTABLE_SUFFIX}")
	elseif(EXISTS "${NODOS_WORKSPACE_DIR}/nosman${CMAKE_EXECUTABLE_SUFFIX}")
		set(NOSMAN_EXECUTABLE "${NODOS_WORKSPACE_DIR}/nosman${CMAKE_EXECUTABLE_SUFFIX}")
	endif()
endif()
if (NOT EXISTS "${NOSMAN_EXECUTABLE}")
	nos_fatal_error("nosman executable not found. Please set NOSMAN_EXECUTABLE to the path of the nosman executable.")
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

# Includes for modules
include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/FindNodosSDK.cmake)
include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/Projects.cmake)

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