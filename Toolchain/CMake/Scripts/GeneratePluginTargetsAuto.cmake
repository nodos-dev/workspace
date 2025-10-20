# Copyright MediaZ Teknoloji A.S. All Rights Reserved.

function(_nos_generate_plugin_target nos_plugin_file_path common_deps out_target_name)
	message(STATUS "Configuring module ${plugin_name}")
	nos_get_module_info_by_path(${nos_plugin_file_path} plugin_name plugin_version out_json_info)
	
	nos_normalize_plugin_name(${plugin_name} target_name)
	get_filename_component(PLUGIN_DIR "${nos_plugin_file_path}" DIRECTORY)

	# Check source code existence
	if(NOT EXISTS "${PLUGIN_DIR}/Source")
		message("Plugin has no source folder, no target generated")
		return()
	endif()

	nos_find_plugin_sdk_dependency(${out_json_info} plugin_sdk_version)
	if ("${plugin_sdk_version}" STREQUAL "None")
		message(STATUS "Module is not depended on a SDK version, there is no need for a target")
		return()
	endif()

	message(STATUS "Plugin SDK version requested: ${plugin_sdk_version}")

	add_compile_definitions(NOS_DISABLE_DEPRECATED)
	nos_find_plugin_sdk(${plugin_sdk_version} NOS_PLUGIN_SDK_TARGET NOS_SDK_DIR)
    if (NOT DEFINED NOS_SDK_DIR)
        message(FATAL_ERROR "Nodos SDK with version ${plugin_sdk_version} not found, please either install it or choose different version")
    endif()

	nos_find_all_plugin_dependencies(${out_json_info} found_dependency_targets found_dep_dirs found_include_dirs)
    list(APPEND INCLUDE_FOLDERS ${CMAKE_CURRENT_SOURCE_DIR} "${CMAKE_CURRENT_SOURCE_DIR}/Include" "${found_include_dirs}")

    nos_generate_flatbuffers("${CMAKE_CURRENT_SOURCE_DIR}/Types" "${CMAKE_CURRENT_SOURCE_DIR}/Include/${target_name}" "cpp" "${NOS_SDK_DIR}/Types;${found_dep_dirs}" ${target_name}_generated)
    list(APPEND MODULE_DEPENDENCIES_TARGETS ${NOS_PLUGIN_SDK_TARGET} ${target_name}_generated)

	list(APPEND
		MODULE_DEPENDENCIES_TARGETS
    	${found_dependency_targets}
    	${${common_deps}}   # dereference here
	)
	message(STATUS "Module dependencies: ${MODULE_DEPENDENCIES_TARGETS}")
    nos_add_plugin("${target_name}" "${MODULE_DEPENDENCIES_TARGETS}" "${INCLUDE_FOLDERS}")
	if(TARGET "${target_name}")
		message(STATUS "Successfully created target: ${target_name}")
	else()
		nos_fatal_error("Failed to create target: ${target_name}")
	endif()

    #Helpers need C++20
    set_target_properties("${target_name}" PROPERTIES CXX_STANDARD 20)
	set(${out_target_name} ${target_name} PARENT_SCOPE)
endfunction()

function(_nos_process_plugin dir common_dependencies)
	if(NOT IS_DIRECTORY ${dir})
		nos_colored_message(COLOR RED "Can't process plugin because it's not folder")
		return()
	endif()
	
	nos_colored_message(COLOR GREEN "Processing module directory: ${dir}")
	file(GLOB PLUGINS "${dir}/*.nosplugin")

	foreach(plugin ${PLUGINS})
		get_filename_component(plugin_name "${plugin}" NAME_WE)


		set(_old_cmake_source_dir ${CMAKE_CURRENT_SOURCE_DIR})
		# Set current source dir to the plugin's directory for includes
		set(CMAKE_CURRENT_SOURCE_DIR "${dir}")
		
		_nos_generate_plugin_target("${plugin}" ${common_dependencies} plugin_target)
		if(COMMAND "nos_plugin_common_post_target_generation")
			nos_colored_message(COLOR CYAN "Calling post target generation function")
			cmake_language(CALL "nos_plugin_common_post_target_generation" "${plugin_target}")
		endif()

		if(EXISTS "${dir}/CMakeLists.txt")
			nos_colored_message(COLOR GREEN "Including custom cmake file for plugin: ${plugin_name}")
			set(NOS_PLUGIN_TARGET ${plugin_target})
			add_subdirectory("${dir}" "${CMAKE_CURRENT_BINARY_DIR}/ModuleDir_${plugin_target}")
		else()
			MESSAGE(STATUS "Custom cmake file for plugin ${plugin_name} not found at ${dir}/CMakeLists.txt")
			string(FIND "${plugin_target}" "nos" pos)
			if (pos EQUAL 0)
				string(FIND "${plugin_target}" "Sys" sys_pos)
				if(sys_pos EQUAL 3)
					nos_group_targets("${plugin_target}" "NOS Subsystems")
				else()
					nos_group_targets("${plugin_target}" "NOS Plugins")
				endif()
			endif()
		endif()

		set(CMAKE_CURRENT_SOURCE_DIR "${_old_cmake_source_dir}")
	endforeach()
endfunction()


function(_nos_collect_plugin_directories dir common_dependencies)
    get_filename_component(parent_dir "${dir}" DIRECTORY)
    get_filename_component(parent_name "${parent_dir}" NAME)
    if(parent_name STREQUAL "Downloaded")
        message(STATUS "Skipping directory under Downloaded: ${dir}")
        return()
    endif()

	file(GLOB PLUGINS "${dir}/*.nosplugin")

	if(EXISTS "${dir}/Common.cmake")
		message("Found common dependency file: ${dir}/Common.cmake")
		include("${dir}/Common.cmake")
		
		if(COMMAND "nos_plugin_common")
			nos_colored_message(COLOR CYAN "Calling common dependency function")

			cmake_language(CALL "nos_plugin_common" ${dir} "${common_dependencies}")
			message("Found common dependencies: ${${common_dependencies}}")
		else()
			nos_fatal_error("Expected function '${plugin_name}' not found in ${plugin_cmake}")
		endif()
	endif()

	if(PLUGINS)
		message("Found module directory: ${dir}/${subdir}")
		foreach(plugin ${PLUGINS})
    		get_filename_component(plugin_name "${plugin}" NAME)
    		message("Found plugin file: ${plugin_name}")
		endforeach()
		if(EXISTS "${dir}/CMakeLists.txt")
			message("Found custom cmake include file: ${dir}/CMakeLists.txt")
		endif()
		_nos_process_plugin(${dir} ${common_dependencies})
	endif()

	file(GLOB SUBDIRS RELATIVE ${dir} ${dir}/*)

	foreach(subdir ${SUBDIRS})
		if(IS_DIRECTORY ${dir}/${subdir})
			_nos_collect_plugin_directories(
				"${dir}/${subdir}"
				${common_dependencies}  
			)

			# Reload functions
			if(EXISTS "${dir}/Common.cmake")
				include("${dir}/Common.cmake")
			endif()
		endif()
	endforeach()

	include(${CMAKE_CURRENT_SOURCE_DIR}/Scripts/DefaultPluginFunctions.cmake)
endfunction()

set(COMMON_DEPS "" CACHE INTERNAL "All custom cmake listed module directories" FORCE)
foreach(cur_module_dir ${MODULE_DIRS})
	# If relative, should be relative to NODOS_WORKSPACE_DIR
	if(NOT IS_ABSOLUTE ${cur_module_dir})
		set(cur_module_dir "${NODOS_WORKSPACE_DIR}/${cur_module_dir}")
	endif()
	nos_colored_message(COLOR GREEN "Scanning for modules in ${cur_module_dir}")
	_nos_collect_plugin_directories("${cur_module_dir}" COMMON_DEPS)
endforeach()
