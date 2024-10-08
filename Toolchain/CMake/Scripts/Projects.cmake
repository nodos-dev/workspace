# Copyright MediaZ Teknoloji A.S. All Rights Reserved.
function(nos_generate_flatbuffers fbs_folders dst_folder out_language include_folders out_target_name)
	if(NOT DEFINED FLATC_EXECUTABLE)
		message(FATAL_ERROR "Flatbuffers compiler not found. Please set FLATC_EXECUTABLE variable.")
	endif()

	list(APPEND fbs_files)
	foreach(fbs_folder ${fbs_folders})
		file(GLOB_RECURSE files ${fbs_folder}/*.fbs)
		list(APPEND fbs_files ${files})
	endforeach()

	foreach(fbs_file ${fbs_files})
		get_filename_component(fbs_file_name ${fbs_file} NAME_WE)
		set(fbs_out_header "${fbs_file_name}_generated.h")
		set(include_params "")

		foreach(include ${include_folders})
			set(include_params ${include_params} -I ${include})
		endforeach()

		set(generated_file ${dst_folder}/${fbs_out_header})
		message(STATUS "Build Task (${out_target_name}): ${fbs_file} -> ${generated_file}")
		list(APPEND out_list ${generated_file})
		add_custom_command(OUTPUT ${generated_file}
			COMMAND ${FLATC_EXECUTABLE}
			-o ${dst_folder}
			${include_params}
			${fbs_file}
			--${out_language}
			--gen-mutable
			--gen-name-strings
			--gen-object-api
			--gen-compare
			--cpp-std=c++17
			--cpp-static-reflection
			--scoped-enums
			--unknown-json
			--reflect-types
			--reflect-names
			--cpp-include array
			# --force-empty-vectors
			# --force-empty
			# --force-defaults
			--object-prefix "T"
			--object-suffix ""
			DEPENDS ${fbs_file}
			COMMENT "Generating flatbuffers: ${fbs_file} (with ${FLATC_EXECUTABLE})"
			VERBATIM)
	endforeach()
	add_custom_target(${out_target_name} DEPENDS ${out_list})
	set_target_properties(${out_target_name} PROPERTIES FOLDER "Build Tasks")
endfunction()

function(nos_get_files_recursive folder file_suffixes out_files_var)
	# Create a temporary variable to collect files in this call
	set(local_files)

	# Get the list of entries in the current folder
	file(GLOB entries LIST_DIRECTORIES true CONFIGURE_DEPENDS "${folder}/*")

	foreach(entry ${entries})
		if(IS_DIRECTORY ${entry})
			# Recursive call for subdirectory
			nos_get_files_recursive("${entry}" "${file_suffixes}" sub_files)
			list(APPEND local_files ${sub_files})
		else()
			foreach(suffix ${file_suffixes})
				if(entry MATCHES ".*\\${suffix}$")
					list(APPEND local_files ${entry})
				endif()
			endforeach()
		endif()
	endforeach()

	# Set the output variable
	set(${out_files_var} ${local_files} PARENT_SCOPE)
endfunction()

function(nos_get_module_info name version query out_var)
	execute_process(
		COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" info ${name} ${version} --relaxed
		RESULT_VARIABLE nosman_result
		OUTPUT_VARIABLE nosman_output
	)

	if(nosman_result EQUAL 0)
		string(STRIP ${nosman_output} nosman_output)
		string(JSON nos_module_info_query_result GET "${nosman_output}" "${query}")
		set(${out_var} ${nos_module_info_query_result} PARENT_SCOPE)
	else()
		message(FATAL_ERROR "Failed to find Nodos module ${name}-${version} in workspace")
	endif()
endfunction()

function(nos_find_module_path name version out_var)
	nos_get_module_info(${name} ${version} "manifest_path" manifest_path)
	string(STRIP ${manifest_path} manifest_path)
	get_filename_component(module_path ${manifest_path} DIRECTORY)
	cmake_path(SET module_path "${module_path}")
	message(STATUS "Found ${name} ${version}: ${module_path}")
	set(${out_var} ${module_path} PARENT_SCOPE)
endfunction()

function(nos_get_module name version out_target_name)
	if(NOT DEFINED NOSMAN_WORKSPACE_DIR)
		message(FATAL_ERROR "NOSMAN_WORKSPACE_DIR is not defined. Set it to the path of the workspace where modules will be installed.")
	endif()

	message(STATUS "Searching/installing Nodos module ${name} ${version} in workspace")

	# TODO: Download if not exists.
	if(NOSMAN_EXECUTABLE)
		# Install module if not exists, silently
		execute_process(
			COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" install ${name} ${version}
			RESULT_VARIABLE nosman_result
			OUTPUT_QUIET
		)

		if(NOT nosman_result EQUAL 0)
			message(STATUS "Failed to install ${name} ${version} in workspace. Trying to rescan modules.")
			execute_process(
				COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" rescan --fetch-index
				RESULT_VARIABLE nosman_result
				OUTPUT_QUIET
			)
			
			if (NOT nosman_result EQUAL 0)
				message(FATAL_ERROR "Failed to rescan modules in workspace. Please check your NOSMAN_WORKSPACE_DIR and NOSMAN_EXECUTABLE variables.")
			endif()

			message(STATUS "Rescanning modules in workspace succeeded. Trying to install ${name} ${version} again.")
			execute_process(
				COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" install ${name} ${version}
				RESULT_VARIABLE nosman_result
				OUTPUT_QUIET
			)
			if (NOT nosman_result EQUAL 0)
				message(FATAL_ERROR "Failed to install ${name} ${version} in workspace.")
			endif()
		endif()

		execute_process(
			COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" info ${name} ${version} --relaxed
			RESULT_VARIABLE nosman_result
			OUTPUT_VARIABLE nosman_output
		)

		if(nosman_result EQUAL 0)
			string(REPLACE "." "_" target_name ${name})
			string(REPLACE "." "_" version_str ${version})
			string(APPEND target_name "-v${version_str}")
			string(PREPEND target_name "__nos_gen__")

			string(STRIP ${nosman_output} nosman_output)

			set(${out_target_name} ${target_name} PARENT_SCOPE)

			if(TARGET ${target_name})
				message(STATUS "Module ${name}-${version} found in project. Using existing target.")
				return()
			endif()

			if (NOT TARGET ${target_name})
				message(STATUS "Creating target ${target_name} for module ${name}-${version}")
				add_library(${target_name} INTERFACE)
	
				# Get module path
				string(JSON module_path GET "${nosman_output}" "manifest_path")
				get_filename_component(module_path ${module_path} DIRECTORY)
				cmake_path(SET module_path "${module_path}")

				# Add fbs files to target
				nos_get_files_recursive(${module_path} ".fbs" fbs_files)
				list(LENGTH fbs_files fbs_count)
				message(STATUS "Found ${fbs_count} schema files in module ${name}-${version}")
				foreach(fbs_file ${fbs_files})
					message(STATUS "${name}-${version} schema file: ${fbs_file}")
				endforeach()
				target_sources(${target_name} INTERFACE ${fbs_files})
				source_group("Schemas" FILES ${fbs_files})
				
				# Optional: Get "public_include_folder" from JSON output
				string(JSON nos_module_include_folder GET "${nosman_output}" "public_include_folder")
				cmake_path(SET ${target_name}_INCLUDE_DIR "${nos_module_include_folder}")
				message(STATUS "Found ${name} ${version} include folder: ${${target_name}_INCLUDE_DIR}")

				if (${target_name}_INCLUDE_DIR STREQUAL "")
					message(STATUS "No public header files found in module ${name}-${version}.")
				else() 
					message(STATUS "Found public header files in module ${name}-${version}. Adding to target.")
					nos_get_files_recursive(${${target_name}_INCLUDE_DIR} ".h;.hpp;.hxx;.hh;.inl" include_files)
					target_sources(${target_name} PUBLIC ${include_files})
					target_include_directories(${target_name} INTERFACE ${${target_name}_INCLUDE_DIR})
				endif()
				set_target_properties(${target_name} PROPERTIES FOLDER "nosman")
			else()
				message(STATUS "Module ${name}-${version} found in project. Using existing target.")
			endif()
		else()
			message(FATAL_ERROR "Failed to find ${name} ${version} include folder")
		endif()
	else()
		message(FATAL_ERROR "Unable to find nosman. Set NOSMAN_EXECUTABLE to use nos_get_module.")
	endif()
endfunction()

function(nos_add_plugin NAME DEPENDENCIES INCLUDE_FOLDERS)
	project(${NAME})
	message(STATUS "Processing plugin ${NAME}")

	set(source_folder "${CMAKE_CURRENT_SOURCE_DIR}/Source")
	set(public_include_folder "${CMAKE_CURRENT_SOURCE_DIR}/Include")
	set(config_folder "${CMAKE_CURRENT_SOURCE_DIR}/Config")
	set(shaders_folder "${CMAKE_CURRENT_SOURCE_DIR}/Shaders")
	if (NOT EXISTS ${source_folder})
		message(FATAL_ERROR "Nodos CMake helpers for adding a plugin requires a folder named 'Source' at the root. Either manually setup your CMake script or create the 'Source' folder.")
	endif()

	set(source_file_types ".cpp" ".cc" ".cxx" ".c" ".inl" ".h" ".hxx" ".hpp" ".py" ".rc" ".natvis")
	nos_get_files_recursive(${source_folder} "${source_file_types}" SOURCE_FILES)
	if (NOT SOURCE_FILES)
		message(FATAL_ERROR "No source files found in ${source_folder}")
	endif()
	
	set(header_file_types ".h" ".hxx" ".hpp")
	nos_get_files_recursive(${public_include_folder} "${header_file_types}" HEADER_FILES)

	set(config_file_types ".json")
	nos_get_files_recursive(${config_folder} "${config_file_types}" CONFIG_FILES)
	source_group("Config" FILES ${CONFIG_FILES})
	nos_get_files_recursive(${config_folder} ".nosdef" NODE_DEFINITION_FILES)
	source_group("Node Definitions" FILES ${NODE_DEFINITION_FILES})
	nos_get_files_recursive(${config_folder} ".fbs" DATA_TYPE_SCHEMA_FILES)
	source_group("Schemas" FILES ${DATA_TYPE_SCHEMA_FILES})

	set(shader_file_types ".glsl" ".comp" ".frag" ".vert" ".hlsl")
	nos_get_files_recursive(${source_folder} "${shader_file_types}" SHADERS)
	nos_get_files_recursive(${shaders_folder} "${shader_file_types}" SHADERS)
	source_group("Shaders" FILES ${SHADERS})
	set_source_files_properties(${SHADERS} PROPERTIES HEADER_FILE_ONLY TRUE)

	file(GLOB MODULE_CFG_FILE CONFIGURE_DEPENDS "*.noscfg")
	set(INCLUDED_IN_PROJECT ${SOURCE_FILES} ${HEADER_FILES} ${CONFIG_FILES} ${NODE_DEFINITION_FILES} ${DATA_TYPE_SCHEMA_FILES} ${SHADERS} ${MODULE_CFG_FILE})
	add_library(${NAME} MODULE ${INCLUDED_IN_PROJECT})
	set_target_properties(${NAME} PROPERTIES
		PREFIX ""
		LIBRARY_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_DEBUG "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELEASE "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELWITHDEBINFO "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_MINSIZEREL "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
	)

	foreach(source IN LISTS SOURCE_FILES)
		get_filename_component(source_path "${source}" PATH)
		string(REPLACE "${CMAKE_CURRENT_SOURCE_DIR}" "" source_path_compact "${source_path}")
		string(REPLACE "/" "\\" source_path_msvc "${source_path_compact}")
		source_group("${source_path_msvc}" FILES "${source}")
	endforeach()

	foreach(header IN LISTS HEADER_FILES)
		get_filename_component(header_path "${header}" PATH)
		string(REPLACE "${CMAKE_CURRENT_SOURCE_DIR}" "" header_path_compact "${header_path}")
		string(REPLACE "/" "\\" header_path_msvc "${header_path_compact}")
		source_group("${header_path_msvc}" FILES "${header}")
	endforeach()

	target_include_directories(${NAME} PRIVATE ${CMAKE_CURRENT_SOURCE_DIR} ${source_folder} ${public_include_folder} ${INCLUDE_FOLDERS})

	foreach(dependency IN LISTS DEPENDENCIES)
		# If target "dependency" type is UTILITY then add it as a dependency
		if(TARGET ${dependency})
			get_target_property(dependency_type ${dependency} TYPE)
			message(STATUS "${PROJECT_NAME}: Adding dependency ${dependency} of type ${dependency_type}")
			if(dependency_type STREQUAL "UTILITY")
				add_dependencies(${NAME} ${dependency})
			else()
				target_link_libraries(${NAME} PRIVATE ${dependency})
			endif()
		else()
			target_link_libraries(${NAME} PRIVATE ${dependency})
		endif()
	endforeach()
endfunction()

function(nos_add_subsystem NAME DEPENDENCIES INCLUDE_FOLDERS)
	project(${NAME})
	message(STATUS "Processing subsystem ${NAME}")

	set(source_folder "${CMAKE_CURRENT_SOURCE_DIR}/Source")
	set(public_include_folder "${CMAKE_CURRENT_SOURCE_DIR}/Include")
	set(config_folder "${CMAKE_CURRENT_SOURCE_DIR}/Config")
	set(shaders_folder "${CMAKE_CURRENT_SOURCE_DIR}/Shaders")
	if (NOT EXISTS ${source_folder})
		message(FATAL_ERROR "Nodos CMake helpers for adding a subsystem requires a folder named 'Source' at the root. Either manually setup your CMake script or create the 'Source' folder.")
	endif()

	set(source_file_types ".cpp" ".cc" ".cxx" ".c" ".inl" ".h" ".hxx" ".hpp" ".py" ".rc" ".natvis")
	nos_get_files_recursive(${source_folder} "${source_file_types}" SOURCE_FILES)
	if (NOT SOURCE_FILES)
		message(FATAL_ERROR "No source files found in ${source_folder}")
	endif()
	
	set(header_file_types ".h" ".hxx" ".hpp")
	nos_get_files_recursive(${public_include_folder} "${header_file_types}" HEADER_FILES)

	set(config_file_types ".json")
	nos_get_files_recursive(${config_folder} "${config_file_types}" CONFIG_FILES)
	source_group("Config" FILES ${CONFIG_FILES})
	nos_get_files_recursive(${config_folder} ".fbs" DATA_TYPE_SCHEMA_FILES)
	source_group("Schemas" FILES ${DATA_TYPE_SCHEMA_FILES})

	set(shader_file_types ".glsl" ".comp" ".frag" ".vert" ".hlsl")
	nos_get_files_recursive(${source_folder} "${shader_file_types}" SHADERS)
	nos_get_files_recursive(${shaders_folder} "${shader_file_types}" SHADERS)
	source_group("Shaders" FILES ${SHADERS})
	set_source_files_properties(${SHADERS} PROPERTIES HEADER_FILE_ONLY TRUE)

	file(GLOB MODULE_CFG_FILE CONFIGURE_DEPENDS "*.nossys")
	set(INCLUDED_IN_PROJECT ${SOURCE_FILES} ${HEADER_FILES} ${CONFIG_FILES} ${DATA_TYPE_SCHEMA_FILES} ${SHADERS} ${MODULE_CFG_FILE})

	add_library(${NAME} MODULE ${INCLUDED_IN_PROJECT})
	set_target_properties(${NAME} PROPERTIES
		PREFIX ""
		LIBRARY_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_DEBUG "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELEASE "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELWITHDEBINFO "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_MINSIZEREL "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
	)

	foreach(source IN LISTS SOURCE_FILES)
		get_filename_component(source_path "${source}" PATH)
		string(REPLACE "${CMAKE_CURRENT_SOURCE_DIR}" "" source_path_compact "${source_path}")
		string(REPLACE "/" "\\" source_path_msvc "${source_path_compact}")
		source_group("${source_path_msvc}" FILES "${source}")
	endforeach()

	foreach(header IN LISTS HEADER_FILES)
		get_filename_component(header_path "${header}" PATH)
		string(REPLACE "${CMAKE_CURRENT_SOURCE_DIR}" "" header_path_compact "${header_path}")
		string(REPLACE "/" "\\" header_path_msvc "${header_path_compact}")
		source_group("${header_path_msvc}" FILES "${header}")
	endforeach()

	target_include_directories(${NAME} PRIVATE  ${CMAKE_CURRENT_SOURCE_DIR} ${source_folder} ${public_include_folder} ${INCLUDE_FOLDERS})

	foreach(dependency IN LISTS DEPENDENCIES)
		# If target "dependency" type is UTILITY then add it as a dependency
		if(TARGET ${dependency})
			get_target_property(dependency_type ${dependency} TYPE)
			message(STATUS "${PROJECT_NAME}: Adding dependency ${dependency} of type ${dependency_type}")
			if(dependency_type STREQUAL "UTILITY")
				add_dependencies(${NAME} ${dependency})
			else()
				target_link_libraries(${NAME} PRIVATE ${dependency})
			endif()
		else()
			target_link_libraries(${NAME} PRIVATE ${dependency})
		endif()
	endforeach()
endfunction()

macro(nos_get_targets targets dir)
    get_property(subdirectories DIRECTORY ${dir} PROPERTY SUBDIRECTORIES)
    foreach(subdir ${subdirectories})
        nos_get_targets(${targets} ${subdir})
    endforeach()
    get_property(current_targets DIRECTORY ${dir} PROPERTY BUILDSYSTEM_TARGETS)
    foreach(subtarget ${current_targets}) 
        if(TARGET ${subtarget})
            list(APPEND ${targets} ${subtarget})
        endif()
    endforeach()
endmacro()

macro(nos_group_targets targets folder_name)
    foreach(target ${targets})
        get_target_property(FOLD ${target} FOLDER)
        if(${FOLD} STREQUAL "FOLD-NOTFOUND")
            set(FOLD_NAME "${folder_name}")
        else()
            set(FOLD_NAME "${folder_name}/${FOLD}")
        endif()
        set_target_properties(${target} PROPERTIES FOLDER ${FOLD_NAME})
    endforeach()
endmacro()