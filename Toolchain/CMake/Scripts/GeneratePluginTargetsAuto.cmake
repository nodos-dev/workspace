# Copyright MediaZ Teknoloji A.S. All Rights Reserved.

function(_nos_get_custom_type_paths_from_json JSON_FILE OUT_LIST)
    if(NOT EXISTS "${JSON_FILE}")
        message(FATAL_ERROR "JSON file not found: ${JSON_FILE}")
    endif()

    # Read file
    file(READ "${JSON_FILE}" _json_content)

    # Check if field exists
    string(JSON _has_custom_types ERROR_VARIABLE _err
        GET "${_json_content}" custom_types
    )

    if(_err)
        # Field does not exist → return empty list
		if (EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/Types)
        	set(${OUT_LIST} "${CMAKE_CURRENT_SOURCE_DIR}/Types" PARENT_SCOPE)
		else()
        	set(${OUT_LIST} "" PARENT_SCOPE)
		endif()
        return()
    endif()

    # Get array length
    string(JSON _len LENGTH "${_json_content}" custom_types)

    set(_result "")
    math(EXPR _last "${_len} - 1")

    foreach(i RANGE 0 ${_last})
        string(JSON _value GET "${_json_content}" custom_types ${i})
        list(APPEND _result "${CMAKE_CURRENT_SOURCE_DIR}/${_value}")
    endforeach()

    set(${OUT_LIST} "${_result}" PARENT_SCOPE)
endfunction()

function(_nos_generate_plugin_target nos_plugin_file_path common_deps common_defs out_target_name out_plugin_name)
	message(STATUS "Configuring plugin ${plugin_name}")
	nos_get_package_info_by_path(${nos_plugin_file_path} plugin_name plugin_version out_json_info)
	
	nos_normalize_plugin_name(${plugin_name} target_name)
	get_filename_component(PLUGIN_DIR "${nos_plugin_file_path}" DIRECTORY)

	nos_find_plugin_sdk_version(${out_json_info} plugin_sdk_version)
	if ("${plugin_sdk_version}" STREQUAL "None")
		message(STATUS "Module is not depended on a SDK version, there is no need for a target")
		return()
	endif()

	message(STATUS "Plugin SDK version requested: ${plugin_sdk_version}")

	nos_find_plugin_sdk(${plugin_sdk_version} NOS_PLUGIN_SDK_TARGET NOS_SDK_DIR)
    if (NOT DEFINED NOS_SDK_DIR)
        message(FATAL_ERROR "Nodos SDK with version ${plugin_sdk_version} not found, please either install it or choose different version")
    endif()

	nos_find_immediate_plugin_dependencies(${out_json_info} found_dependency_targets found_dep_dirs found_include_dirs)
    list(APPEND INCLUDE_FOLDERS ${CMAKE_CURRENT_SOURCE_DIR} "${CMAKE_CURRENT_SOURCE_DIR}/Include" "${found_include_dirs}")
    list(APPEND MODULE_DEPENDENCIES_TARGETS ${NOS_PLUGIN_SDK_TARGET})

	_nos_get_custom_type_paths_from_json(${nos_plugin_file_path} TYPE_FOLDERS)
	if(TYPE_FOLDERS)
    	nos_generate_flatbuffers("${TYPE_FOLDERS}" "${CMAKE_CURRENT_SOURCE_DIR}/Include/${target_name}" "cpp" "${NOS_SDK_DIR}/Types;${found_dep_dirs}" ${target_name}_generated)
    	list(APPEND MODULE_DEPENDENCIES_TARGETS ${target_name}_generated)
	endif()

	list(APPEND
		MODULE_DEPENDENCIES_TARGETS
    	${found_dependency_targets}
    	${common_deps}   # dereference here
	)
	message(STATUS "Module dependencies: ${MODULE_DEPENDENCIES_TARGETS}")
    nos_add_plugin("${target_name}" "${MODULE_DEPENDENCIES_TARGETS}" "${INCLUDE_FOLDERS}")
	if(TARGET "${target_name}")
		message(STATUS "Successfully created target: ${target_name}")
		set(${out_target_name} ${target_name} PARENT_SCOPE)
		set(${out_plugin_name} ${plugin_name} PARENT_SCOPE)

		get_target_property(target_type ${target_name} TYPE)
		if(target_type STREQUAL "INTERFACE_LIBRARY")
			return()
		endif()
	else()
		nos_fatal_error("Failed to create target: ${target_name}")
	endif()

    #Helpers need C++20
    set_target_properties("${target_name}" PROPERTIES CXX_STANDARD 20)
	target_compile_definitions("${target_name}" PRIVATE ${common_defs})
		
	if(NOS_FORCE_DISABLE_DEPRECATED)
		target_compile_definitions("${target_name}" PRIVATE NOS_DISABLE_DEPRECATED)
	endif()
endfunction()

function(_nos_configure_plugin_dir dir common_dependencies common_definitions)
	if(NOT IS_DIRECTORY ${dir})
		nos_colored_message(COLOR RED "Can't process plugin because it's not folder: ${dir}")
		return()
	endif()
	
	nos_colored_message(COLOR GREEN "Processing plugin directory: ${dir}")
	file(GLOB PLUGINS "${dir}/*.nosplugin")

	foreach(plugin ${PLUGINS})
		get_filename_component(plugin_name "${plugin}" NAME_WE)

		set(_old_cmake_source_dir ${CMAKE_CURRENT_SOURCE_DIR})
		# Set current source dir to the plugin's directory for includes
		set(CMAKE_CURRENT_SOURCE_DIR "${dir}")
		
		_nos_generate_plugin_target("${plugin}" "${common_dependencies}" "${common_definitions}" plugin_target plugin_name)
		if(NOT TARGET ${plugin_target})
			return()
		endif()

		if(EXISTS "${dir}/CMakeLists.txt")
			nos_colored_message(COLOR GREEN "Including custom cmake file for plugin: ${plugin_name}")
			set(NOS_PLUGIN_TARGET ${plugin_target})
			add_subdirectory("${dir}" "${CMAKE_CURRENT_BINARY_DIR}/ModuleDir_${plugin_target}")
		endif()
		nos_get_vendor_name(${plugin_name} plugin_vendor)
		nos_group_targets(${plugin_target} "${plugin_vendor} Plugins")

		if(COMMAND "nos_plugin_on_post_target_generated")
			nos_colored_message(COLOR CYAN "Calling post target generation function")
			cmake_language(CALL "nos_plugin_on_post_target_generated" "${plugin_target}" "${plugin_name}")
		endif()

		set(CMAKE_CURRENT_SOURCE_DIR "${_old_cmake_source_dir}")
	endforeach()
endfunction()


function(_nos_process_plugin_directories_recursive dir common_dependencies common_definitions)
    get_filename_component(parent_dir "${dir}" DIRECTORY)
    get_filename_component(parent_name "${parent_dir}" NAME)
    if(parent_name STREQUAL "Downloaded")
        message(STATUS "Skipping directory under Downloaded: ${dir}")
        return()
    endif()

	file(GLOB PLUGINS "${dir}/*.nosplugin")

	if(EXISTS "${dir}/NosPluginCommon.cmake")
		message("Found common dependency file: ${dir}/NosPluginCommon.cmake")
		include("${dir}/NosPluginCommon.cmake")
		
		if(COMMAND "nos_plugin_common")
			nos_colored_message(COLOR CYAN "Calling common dependency function")

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
    	list(GET PLUGINS 0 plugin)
		get_filename_component(plugin_name "${plugin}" NAME)
		
		_nos_configure_plugin_dir(${dir} "${common_dependencies}" "${common_definitions}")
	endif()

	file(GLOB SUBDIRS RELATIVE ${dir} ${dir}/*)

	foreach(subdir ${SUBDIRS})
		include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/DefaultNosPluginCommon.cmake)
		_nos_process_plugin_directories_recursive(
			"${dir}/${subdir}"
			"${common_dependencies}"  
			"${common_definitions}"
		)

		# Reload functions
		if(EXISTS "${dir}/NosPluginCommon.cmake")
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