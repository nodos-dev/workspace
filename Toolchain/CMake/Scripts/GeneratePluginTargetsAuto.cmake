# Copyright MediaZ Teknoloji A.S. All Rights Reserved.

function(_nos_get_custom_type_paths_from_json JSON_FILE OUT_LIST)
	if(NOT EXISTS "${JSON_FILE}")
		message(FATAL_ERROR "JSON file not found: ${JSON_FILE}")
	endif()

	# Read file
	file(READ "${JSON_FILE}" json_content)

	# Check if field exists
	string(JSON has_custom_types ERROR_VARIABLE err
		GET "${json_content}" custom_types
	)

	if(err)
		# Field does not exist → return empty list
		if (EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/Types)
			set(${OUT_LIST} "${CMAKE_CURRENT_SOURCE_DIR}/Types" PARENT_SCOPE)
		else()
			set(${OUT_LIST} "" PARENT_SCOPE)
		endif()
		return()
	endif()

	# Get array length
	string(JSON len LENGTH "${json_content}" custom_types)
	if ("${len}" STREQUAL "0")
		set(${OUT_LIST} "" PARENT_SCOPE)
		return()
	endif()

	set(result "")
	math(EXPR last "${len} - 1")

	foreach(i RANGE 0 ${last})
		string(JSON value GET "${json_content}" custom_types ${i})
		list(APPEND result "${CMAKE_CURRENT_SOURCE_DIR}/${value}")
	endforeach()

	set(${OUT_LIST} "${result}" PARENT_SCOPE)
endfunction()

function(_nos_generate_plugin_target plugin_manifest_file_path plugin_name manifest_json common_deps common_defs out_target_name)
	
	nos_normalize_plugin_name(${plugin_name} target_name)

	string(JSON plugin_sdk_version ERROR_VARIABLE err GET "${manifest_json}" sdk_version)
	if (err)
		nos_fatal_error("Failed to read used SDK version from plugin ${plugin_manifest_file_path}: ${err}")
	endif()
	if ("${plugin_sdk_version}" STREQUAL "None")
		nos_message(STATUS "Plugin is not depended on a SDK version, there is no need for a target")
		return()
	endif()

	nos_message(STATUS "Plugin SDK version requested: ${plugin_sdk_version}")

	nos_find_plugin_sdk(${plugin_sdk_version} NOS_PLUGIN_SDK_TARGET NOS_SDK_DIR)
	if (NOT DEFINED NOS_SDK_DIR)
		message(FATAL_ERROR "Nodos SDK with version ${plugin_sdk_version} not found, please either install it or choose different version")
	endif()

	nos_find_immediate_plugin_dependencies(${manifest_json} found_dependency_targets found_dep_dirs found_include_dirs)
	list(APPEND plugin_include_folders ${CMAKE_CURRENT_SOURCE_DIR} "${CMAKE_CURRENT_SOURCE_DIR}/Include" "${found_include_dirs}")
	list(APPEND plugin_dep_targets ${NOS_PLUGIN_SDK_TARGET})

	_nos_get_custom_type_paths_from_json(${plugin_manifest_file_path} TYPE_FOLDERS)
	if(TYPE_FOLDERS)
		set(generated_include_root "${CMAKE_CURRENT_BINARY_DIR}/__generated__/${target_name}/Include")
		set(generated_include_dir "${generated_include_root}/${target_name}")
		nos_generate_flatbuffers("${TYPE_FOLDERS}" "${generated_include_dir}" "cpp" "${NOS_SDK_DIR}/Types;${found_dep_dirs}" ${target_name}_generated)
		list(APPEND plugin_include_folders "${generated_include_root}")
		list(APPEND plugin_dep_targets ${target_name}_generated)
	endif()

	list(APPEND
		plugin_dep_targets
		${found_dependency_targets}
		${common_deps}   # dereference here
	)
	nos_message(STATUS "Plugin dependencies: ${plugin_dep_targets}")
	nos_add_plugin("${target_name}" "${plugin_dep_targets}" "${plugin_include_folders}")
	if(TARGET "${target_name}")
		nos_message(STATUS "Successfully created target: ${target_name}")
		set(${out_target_name} ${target_name} PARENT_SCOPE)
		set(${out_plugin_name} ${plugin_name} PARENT_SCOPE)

		get_target_property(target_type ${target_name} TYPE)
		if(target_type STREQUAL "INTERFACE_LIBRARY")
			return()
		endif()
	else()
		nos_fatal_error("Failed to create target: ${target_name}")
	endif()

	# Helpers in Nodos SDK need C++20
	set_target_properties("${target_name}" PROPERTIES CXX_STANDARD 20)
	target_compile_definitions("${target_name}" PRIVATE ${common_defs})
		
	if(NOS_FORCE_DISABLE_DEPRECATED)
		target_compile_definitions("${target_name}" PRIVATE NOS_DISABLE_DEPRECATED)
	endif()
endfunction()

function(_nos_configure_plugin plugin_manifest_file common_dependencies common_definitions)
	nos_get_package_info_by_path(${plugin_manifest_file} plugin_name plugin_version manifest_json)
	nos_colored_message(COLOR CYAN "Configuring ${plugin_name} (${plugin_version})")

	set(old_cmake_source_dir ${CMAKE_CURRENT_SOURCE_DIR})
	# Set current source dir to the plugin's directory for includes
	set(CMAKE_CURRENT_SOURCE_DIR "${dir}")
	
	_nos_generate_plugin_target("${plugin_manifest_file}" "${plugin_name}" "${manifest_json}" "${common_dependencies}" "${common_definitions}" plugin_target)
	if (NOT TARGET ${plugin_target})
		return()
	endif()

	if (EXISTS "${dir}/CMakeLists.txt")
		nos_colored_message(DIMMED COLOR CYAN "Including custom CMake file for plugin: ${plugin_name}")
		set(NOS_PLUGIN_TARGET ${plugin_target})
		add_subdirectory("${dir}" "${CMAKE_CURRENT_BINARY_DIR}/PluginDir_${plugin_target}")
	endif()
	nos_get_short_vendor_name(${plugin_name} plugin_vendor)
	string(TOUPPER "${plugin_vendor}" plugin_vendor_upper)
	nos_group_targets(${plugin_target} "${plugin_vendor_upper} Plugins")

	if (COMMAND "nos_plugin_on_post_target_generated")
		cmake_language(CALL "nos_plugin_on_post_target_generated" "${plugin_target}" "${plugin_name}")
	endif()

	set(CMAKE_CURRENT_SOURCE_DIR "${old_cmake_source_dir}")
endfunction()


function(_nos_process_plugin_directories_recursive dir common_dependencies common_definitions)
	get_filename_component(parent_dir "${dir}" DIRECTORY)
	get_filename_component(parent_name "${parent_dir}" NAME)
	if (parent_name STREQUAL "Downloaded")
		nos_message(STATUS "Skipping directory under Downloaded: ${dir}")
		return()
	endif()

	file(GLOB PLUGINS "${dir}/*.nosplugin")

	if (EXISTS "${dir}/NosPluginCommon.cmake")
		nos_message("Found common dependency file: ${dir}/NosPluginCommon.cmake")
		include("${dir}/NosPluginCommon.cmake")
		
		if (COMMAND "nos_plugin_common")
			# Print relative dir to workspace for better readability
			file(RELATIVE_PATH rel_dir "${NODOS_WORKSPACE_DIR}" "${dir}")
			nos_colored_message(COLOR CYAN "Calling common CMake function for directory: ${rel_dir}")

			set(common_deps "")
			set(common_defs "")
			cmake_language(CALL "nos_plugin_common" ${dir} common_deps common_defs)
			list(APPEND common_dependencies ${common_deps})
			list(APPEND common_definitions ${common_defs})
		else()
			nos_fatal_error("Expected function '${plugin_name}' not found in ${plugin_cmake}")
		endif()
	endif()

	list(LENGTH PLUGINS PLUGIN_COUNT)

	if (PLUGIN_COUNT GREATER 1)
		nos_fatal_error("Multiple .nosplugin files found in directory: ${dir}")
	elseif (PLUGIN_COUNT EQUAL 1)
		list(GET PLUGINS 0 plugin_manifest_filepath)
		get_filename_component(plugin_name "${plugin_manifest_filepath}" NAME)
		
		_nos_configure_plugin(${plugin_manifest_filepath} "${common_dependencies}" "${common_definitions}")
	endif()

	file(GLOB SUBDIRS RELATIVE ${dir} ${dir}/*)

	foreach(subdir ${SUBDIRS})
		include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/DefaultNosPluginCommon.cmake)
		
		# try to find a *.nosplugin, if not, skip
		# TODO: Ideally, this should done only at the start and only the directories containing plugins should be processed
		file(GLOB_RECURSE FOUND_PLUGINS RELATIVE "${dir}" "${dir}/${subdir}/*.nosplugin")
		if (NOT FOUND_PLUGINS)
			continue()
		endif()
		_nos_process_plugin_directories_recursive(
			"${dir}/${subdir}"
			"${common_dependencies}"  
			"${common_definitions}"
		)

		# Reload functions
		if (EXISTS "${dir}/NosPluginCommon.cmake")
			include("${dir}/NosPluginCommon.cmake")
		endif()
	endforeach()

	include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/DefaultNosPluginCommon.cmake)
endfunction()

foreach(cur_plugin_dir ${MODULE_DIRS})
	# If relative, should be relative to NODOS_WORKSPACE_DIR
	if(NOT IS_ABSOLUTE ${cur_plugin_dir})
		set(cur_plugin_dir "${NODOS_WORKSPACE_DIR}/${cur_plugin_dir}")
	endif()
	nos_colored_message(COLOR GREEN "Scanning for plugins in ${cur_plugin_dir}")
	_nos_process_plugin_directories_recursive("${cur_plugin_dir}" "" "")
endforeach()
