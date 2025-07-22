# Copyright MediaZ Teknoloji A.S. All Rights Reserved.
function(nos_generate_flatbuffers fbs_folders dst_folder out_language include_folders out_target_name)
	if(NOT DEFINED FLATC_EXECUTABLE)
		nos_fatal_error("Flatbuffers compiler not found. Please set FLATC_EXECUTABLE variable.")
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

	foreach(suffix ${file_suffixes})
		#find every file, not directories
		file(GLOB_RECURSE entries CONFIGURE_DEPENDS "${folder}/*${suffix}")
		list(APPEND local_files ${entries})
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
		string(JSON nos_module_info_query_result ERROR_VARIABLE err GET "${nosman_output}" "${query}")
		if(err STREQUAL "NOTFOUND")
			set(${out_var} ${nos_module_info_query_result} PARENT_SCOPE)
		else()
			nos_fatal_error("Failed to get info '${query}' from module ${name}-${version}.")
			set(${out_var} "NOTFOUND")
		endif()
	else()
		nos_fatal_error("Failed to find Nodos module ${name}-${version} in workspace")
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
		nos_fatal_error("NOSMAN_WORKSPACE_DIR is not defined. Set it to the path of the workspace where modules will be installed.")
	endif()

	string(REPLACE "." "_" target_name ${name})
	string(REPLACE "." "_" version_str ${version})
	string(APPEND target_name "-v${version_str}")
	string(PREPEND target_name "__nos_gen__")

	set(${out_target_name} ${target_name} PARENT_SCOPE)

	if(TARGET ${target_name})
		message(STATUS "Module ${name}-${version} already found in project. Using existing target.")
		return()
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
				nos_fatal_error("Failed to rescan modules in workspace. Please check your NOSMAN_WORKSPACE_DIR and NOSMAN_EXECUTABLE variables.")
			endif()

			message(STATUS "Rescanning modules in workspace succeeded. Trying to install ${name} ${version} again.")
			execute_process(
				COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" install ${name} ${version}
				RESULT_VARIABLE nosman_result
				OUTPUT_QUIET
			)
			if (NOT nosman_result EQUAL 0)
				nos_fatal_error("Failed to install ${name} ${version} in workspace.")
			endif()
		endif()

		execute_process(
			COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" info ${name} ${version} --relaxed
			RESULT_VARIABLE nosman_result
			OUTPUT_VARIABLE nosman_output
		)

		if(nosman_result EQUAL 0)
			string(STRIP ${nosman_output} nosman_output)

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
			source_group("Types" FILES ${fbs_files})
			
			# Optional: Get "public_include_folder" from JSON output. If not found skip it
			string(JSON nos_module_include_folder ERROR_VARIABLE err GET "${nosman_output}" "public_include_folder")
			if (err STREQUAL "NOTFOUND")
				message(STATUS "Found ${name} ${version} include folder: ${nos_module_include_folder}")
				cmake_path(SET ${target_name}_INCLUDE_DIR "${nos_module_include_folder}")
				message(STATUS "Found public header files in module ${name}-${version}. Adding to target.")
				nos_get_files_recursive(${${target_name}_INCLUDE_DIR} ".h;.hpp;.hxx;.hh;.inl" include_files)
				target_sources(${target_name} PUBLIC ${include_files})
				target_include_directories(${target_name} INTERFACE ${${target_name}_INCLUDE_DIR})
			else()
				message(STATUS "No public header files found in module ${name}-${version}.")
			endif()
			set_target_properties(${target_name} PROPERTIES FOLDER "nosman")
		else()
			nos_fatal_error("Failed to find ${name} ${version} include folder")
		endif()
	else()
		nos_fatal_error("Unable to find nosman. Set NOSMAN_EXECUTABLE to use nos_get_module.")
	endif()
endfunction()

function(_nos_add_module NAME INCLUDE_FOLDERS MANIFEST_FILE_EXT ADDITIONAL_FILE_TYPES ALTERNATIVE_MANIFEST_FILE_EXTS)
	project(${NAME})
	nos_colored_message(COLOR CYAN "Processing plugin ${NAME}")

	set(module_root "${CMAKE_CURRENT_SOURCE_DIR}")
	set(config_folder "${module_root}/Config")
	set(source_folder "${module_root}/Source")
	set(public_include_folder "${module_root}/Include")
	set(shaders_folder "${module_root}/Shaders")
	if (NOT EXISTS ${source_folder})
		nos_fatal_error("Nodos CMake helpers for adding a plugin requires a folder named 'Source' at the root. Either manually setup your CMake script or create the 'Source' folder.")
	endif()

	set(source_file_types ".cpp" ".cc" ".cxx" ".c" ".inl" ".h" ".hxx" ".hpp" ".py" ".rc" ".natvis")
	nos_get_files_recursive(${source_folder} "${source_file_types}" source_files)
	if (NOT source_files)
		nos_fatal_error("No source files found in ${source_folder}")
	endif()
	
	set(header_file_types ".h" ".hxx" ".hpp")
	nos_get_files_recursive(${public_include_folder} "${header_file_types}" header_files)

	set(config_file_types ".json")
	nos_get_files_recursive(${config_folder} "${config_file_types}" config_files)
	source_group("Config" FILES ${config_files})
	nos_get_files_recursive(${module_root} ".fbs" type_schema_files)
	source_group("Types" FILES ${type_schema_files})

	list(LENGTH ADDITIONAL_FILE_TYPES len_file_types_list)
	math(EXPR last_idx "${len_file_types_list} - 1")
	
	list(APPEND additional_files)
	if (last_idx GREATER 0)
		foreach(file_idx RANGE 0 ${last_idx} 2)
			unset(_files)
			math(EXPR group_idx "${file_idx} + 1")
			list(GET ADDITIONAL_FILE_TYPES ${file_idx} file_type)
			list(GET ADDITIONAL_FILE_TYPES ${group_idx} group_name)
			message(STATUS "Adding file type ${file_type} in source group ${group_name}")
			nos_get_files_recursive(${module_root} ${file_type} _files)
			source_group("${group_name}" FILES ${_files})
			foreach(file IN LISTS _files)
				list(APPEND additional_files ${file})
			endforeach()
		endforeach()
	endif()

	set(shader_file_types ".glsl" ".comp" ".frag" ".vert" ".hlsl")
	nos_get_files_recursive(${module_root} "${shader_file_types}" shader_files)
	nos_get_files_recursive(${shaders_folder} "${shader_file_types}" shader_files)
	source_group("Shaders" FILES ${shader_files})
	set_source_files_properties(${shader_files} PROPERTIES HEADER_FILE_ONLY TRUE)

	file(GLOB MODULE_MANIFEST_FILE CONFIGURE_DEPENDS "*.${MANIFEST_FILE_EXT}")
	foreach (alternative_manifest_file_ext ${ALTERNATIVE_MANIFEST_FILE_EXTS})
		if (NOT alternative_manifest_file_ext STREQUAL "")
			file(GLOB ALTERNATIVE_MODULE_MANIFEST_FILES CONFIGURE_DEPENDS "*.${alternative_manifest_file_ext}")
			list(APPEND MODULE_MANIFEST_FILE ${ALTERNATIVE_MODULE_MANIFEST_FILES})
		endif()
	endforeach()
	set(INCLUDED_IN_PROJECT ${source_files} ${header_files} ${config_files} ${NODE_DEFINITION_FILES} ${type_schema_files} ${shader_files} ${additional_files} ${MODULE_MANIFEST_FILE} ${ALTERNATIVE_MODULE_MANIFEST_FILES})
	add_library(${NAME} MODULE ${INCLUDED_IN_PROJECT})
	set_target_properties(${NAME} PROPERTIES
		PREFIX ""
		LIBRARY_OUTPUT_DIRECTORY "${module_root}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_DEBUG "${module_root}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELEASE "${module_root}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELWITHDEBINFO "${module_root}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_MINSIZEREL "${module_root}/Binaries"
	)

	foreach(source IN LISTS source_files)
		get_filename_component(source_path "${source}" PATH)
		string(REPLACE "${module_root}" "" source_path_compact "${source_path}")
		string(REPLACE "/" "\\" source_path_msvc "${source_path_compact}")
		source_group("${source_path_msvc}" FILES "${source}")
	endforeach()

	foreach(header IN LISTS header_files)
		get_filename_component(header_path "${header}" PATH)
		string(REPLACE "${module_root}" "" header_path_compact "${header_path}")
		string(REPLACE "/" "\\" header_path_msvc "${header_path_compact}")
		source_group("${header_path_msvc}" FILES "${header}")
	endforeach()

	target_include_directories(${NAME} PRIVATE ${module_root} ${source_folder} ${public_include_folder} ${INCLUDE_FOLDERS})

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

	# Produce PDBs in release mode too
	if (CMAKE_BUILD_TYPE STREQUAL "Release")
		if (MSVC)
			target_compile_options(${NAME} PRIVATE /Zi)
			target_link_options(${NAME} PRIVATE /DEBUG /OPT:REF /OPT:ICF)
		endif()
	endif()
endfunction()

function(nos_add_plugin NAME DEPENDENCIES INCLUDE_FOLDERS)
	_nos_add_module(${NAME} "${INCLUDE_FOLDERS}" "noscfg" ".nosdef;Node Definitions;.nosnode;Node Definitions" "nossys;nosplugin")
endfunction()

function(nos_add_subsystem NAME DEPENDENCIES INCLUDE_FOLDERS)
	_nos_add_module(${NAME} "${INCLUDE_FOLDERS}" "nossys" "" "nosplugin")
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