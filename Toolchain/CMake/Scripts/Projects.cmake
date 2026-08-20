# Copyright MediaZ Teknoloji A.S. All Rights Reserved.
set(NOS_SOURCE_FILE_TYPES ".cpp" ".cc" ".cxx" ".c" ".inl" ".h" ".hxx" ".hpp" ".py" ".rc")
set(NOS_HEADER_FILE_TYPES ".h" ".hxx" ".hpp" ".natvis")

function(nos_generate_flatbuffers fbs_paths dst_folder out_language include_folders out_target_name)
	if(NOT DEFINED FLATC_EXECUTABLE)
		nos_fatal_error("Flatbuffers compiler not found. Please set FLATC_EXECUTABLE variable.")
	endif()

	# Ensure NOS_SDK_TYPES_DIR is always included regardless of caller-provided paths
	if(DEFINED NOS_SDK_TYPES_DIR)
		list(APPEND include_folders "${NOS_SDK_TYPES_DIR}")
		list(REMOVE_DUPLICATES include_folders)
	endif()

	list(APPEND fbs_files)
	foreach (fbs_path ${fbs_paths})
		if (EXISTS "${fbs_path}")
			if (IS_DIRECTORY "${fbs_path}")
				file(GLOB_RECURSE files ${fbs_path}/*.fbs)
				list(APPEND fbs_files ${files})
			else ()
				list(APPEND fbs_files ${fbs_path})
			endif()
		else()
			nos_fatal_error("Flatbuffers schema path doesn't exist: ${fbs_path}")
		endif()

		if(NOT EXISTS ${folder})
		endif()
	endforeach()

	# Ensure destination directory exists
	file(MAKE_DIRECTORY ${dst_folder})

	# Prepare common flatc arguments
	set(flatc_common_args
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
	)

	foreach(fbs_file ${fbs_files})
		get_filename_component(fbs_file_name ${fbs_file} NAME_WE)
		set(fbs_out_header "${fbs_file_name}_generated.h")
		set(include_params)

		foreach(include ${include_folders})
			list(APPEND include_params -I ${include})
		endforeach()

		set(generated_file ${dst_folder}/${fbs_out_header})
		
		# Construct complete command list (used for both configure-time and build-time)
		set(flatc_command "${FLATC_EXECUTABLE}")
		# Add include parameters (if any)
		if(include_params)
			list(APPEND flatc_command ${include_params})
		endif()
		list(APPEND flatc_command -o "${dst_folder}")
		list(APPEND flatc_command ${flatc_common_args})
		list(APPEND flatc_command "${fbs_file}")
		
		# Check if we need to generate at configure time
		set(need_generation FALSE)
		if(NOT EXISTS ${generated_file})
			set(need_generation TRUE)
		else()
			# Check if source is newer than generated file using Unix timestamps
			file(TIMESTAMP ${fbs_file} fbs_timestamp "%s")
			file(TIMESTAMP ${generated_file} generated_timestamp "%s")
			if(fbs_timestamp GREATER generated_timestamp)
				set(need_generation TRUE)
			else()
				nos_message(STATUS "${fbs_out_header} is up to date")
			endif()
		endif()

		# Generate at configure time if needed
		if(need_generation)
			nos_colored_message(COLOR MAGENTA "-- Generating ${fbs_out_header}")
			execute_process(
				COMMAND ${flatc_command}
					--object-suffix "" # Workaround for passing empty string to command in CMake
				RESULT_VARIABLE flatc_result
				OUTPUT_VARIABLE flatc_output
				ERROR_VARIABLE flatc_error
			)
			
			if(NOT flatc_result EQUAL 0)
				message(FATAL_ERROR "Failed to generate flatbuffers for ${fbs_file}:\nOutput: ${flatc_output}\nError: ${flatc_error}")
			endif()
		endif()

		nos_message(STATUS "Build Task (${out_target_name}): ${fbs_file} -> ${generated_file}")
		list(APPEND out_list ${generated_file})
		
		add_custom_command(OUTPUT ${generated_file}
			COMMAND ${flatc_command}
				--object-suffix "" # Workaround for passing empty string to command in CMake
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

function(nos_get_package_info name version query out_var)
	execute_process(
		COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" info ${name} ${version} --relaxed
		RESULT_VARIABLE nosman_result
		OUTPUT_VARIABLE nosman_output
	)

	if(nosman_result EQUAL 0)
		string(STRIP ${nosman_output} nosman_output)
		string(JSON nos_plugin_info_query_result ERROR_VARIABLE err GET "${nosman_output}" "${query}")
		if(err STREQUAL "NOTFOUND")
			set(${out_var} ${nos_plugin_info_query_result} PARENT_SCOPE)
		else()
			nos_fatal_error("Failed to get info '${query}' from package ${name}-${version}.")
			set(${out_var} "NOTFOUND")
		endif()
	else()
		nos_fatal_error("Failed to find Nodos package ${name}-${version} in workspace")
	endif()
endfunction()

function(nos_find_package_path name version out_var)
	nos_get_package_info(${name} ${version} "manifest_path" manifest_path)
	string(STRIP ${manifest_path} manifest_path)
	get_filename_component(package_path ${manifest_path} DIRECTORY)
	cmake_path(SET package_path "${package_path}")
	nos_message(STATUS "Found ${name} ${version}: ${package_path}")
	set(${out_var} ${package_path} PARENT_SCOPE)
endfunction()

# Where a package's generated type headers live. Inside the project, so they
# never land in a source folder that is being globbed, and keyed by package so
# whoever needs them can find them without asking the package that owns them.
function(_nos_get_generated_dir name version out_var)
	set(${out_var} "${CMAKE_BINARY_DIR}/Generated/${name}/${version}" PARENT_SCOPE)
endfunction()

# Generates the type headers of a package from the schemas it ships, and returns
# the target that regenerates them plus the folders to include them from.
#
# Nodos 1.4 packages ship their schemas, so every build generates the headers
# with the flatc of the SDK it is building against. Earlier packages ship the
# headers instead, and their schemas are not discoverable, so they are left
# alone and their own Include folder serves.
#
# Calling this twice for the same package is fine: the first call generates and
# the rest get the same target back.
#
# TODO: A package published from a working copy that still has the old headers
# under Include will ship them, and they then compete with the generated ones on
# a consumer's include path. Exclude *_generated.h from 1.4 releases in nosman.
function(_nos_generate_package_types package_json out_target_name out_include_dirs)
	set(${out_target_name} "" PARENT_SCOPE)
	set(${out_include_dirs} "" PARENT_SCOPE)

	string(JSON package_name GET "${package_json}" info id name)
	string(JSON package_version GET "${package_json}" info id version)
	string(JSON manifest_path GET "${package_json}" manifest_path)

	nos_normalize_plugin_name(${package_name} types_folder_name)
	_nos_get_generated_dir(${package_name} ${package_version} generated_dir)
	set(include_dirs "${generated_dir}" "${generated_dir}/${types_folder_name}")

	string(REPLACE "." "_" target_name "__nos_types__${package_name}-v${package_version}")
	if (TARGET ${target_name})
		set(${out_target_name} ${target_name} PARENT_SCOPE)
		set(${out_include_dirs} "${include_dirs}" PARENT_SCOPE)
		return()
	endif()

	get_filename_component(manifest_ext "${manifest_path}" LAST_EXT)
	if (NOT manifest_ext STREQUAL ".nosplugin")
		nos_message(STATUS "Package ${package_name}-${package_version} predates Nodos 1.4, using the type headers it ships")
		return()
	endif()

	string(JSON schema_count ERROR_VARIABLE err LENGTH "${package_json}" type_schema_files)
	if (err OR schema_count EQUAL 0)
		nos_message(STATUS "Package ${package_name}-${package_version} has no type schemas")
		return()
	endif()

	# Generating a package walks into its dependencies, and a nested call would
	# otherwise keep appending to the list it inherited from this one.
	set(schema_files "")

	math(EXPR last_schema_idx "${schema_count} - 1")
	foreach(idx RANGE ${last_schema_idx})
		string(JSON schema_file GET "${package_json}" type_schema_files ${idx})
		cmake_path(SET schema_file "${schema_file}")
		list(APPEND schema_files "${schema_file}")
	endforeach()

	get_filename_component(package_root "${manifest_path}" DIRECTORY)
	cmake_path(SET package_root "${package_root}")
	set(schema_include_dirs "${package_root}")

	# A package's schemas may include the schemas of the packages it depends on,
	# so flatc needs to be able to find those too.
	string(JSON dep_count ERROR_VARIABLE err LENGTH "${package_json}" info dependencies)
	if (NOT err AND dep_count GREATER 0)
		math(EXPR last_dep_idx "${dep_count} - 1")
		foreach(idx RANGE ${last_dep_idx})
			string(JSON dep_name GET "${package_json}" info dependencies ${idx} name)
			string(JSON dep_version GET "${package_json}" info dependencies ${idx} version)
			nos_get_package(${dep_name} ${dep_version} dep_target)
			get_target_property(dep_root ${dep_target} NOS_PACKAGE_ROOT)
			if (dep_root)
				list(APPEND schema_include_dirs "${dep_root}")
			endif()
		endforeach()
	endif()

	nos_generate_flatbuffers("${schema_files}" "${generated_dir}/${types_folder_name}" "cpp" "${schema_include_dirs}" ${target_name})

	set(${out_target_name} ${target_name} PARENT_SCOPE)
	set(${out_include_dirs} "${include_dirs}" PARENT_SCOPE)
endfunction()

function(nos_get_package name version out_target_name)
	if(NOT DEFINED NOSMAN_WORKSPACE_DIR)
		nos_fatal_error("NOSMAN_WORKSPACE_DIR is not defined. Set it to the path of the workspace where modules will be installed.")
	endif()

	string(REPLACE "." "_" target_name ${name})
	string(REPLACE "." "_" version_str ${version})
	string(APPEND target_name "-v${version_str}")
	string(PREPEND target_name "__nos_gen__")

	set(${out_target_name} ${target_name} PARENT_SCOPE)

	if(TARGET ${target_name})
		nos_message(STATUS "Package ${name}-${version} already found in project. Using existing target.")
		return()
	endif()

	nos_message(STATUS "Searching/installing Nodos package ${name} ${version} in workspace")

	# TODO: Download if not exists.
	if(NOSMAN_EXECUTABLE)
		# Install module if not exists, silently
		execute_process(
			COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" install ${name} ${version}
			RESULT_VARIABLE nosman_result
			OUTPUT_QUIET
		)

		if(NOT nosman_result EQUAL 0)
			nos_message(STATUS "Failed to install ${name} ${version} in workspace. Trying to rescan modules.")
			execute_process(
				COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" rescan --fetch-index
				RESULT_VARIABLE nosman_result
				OUTPUT_QUIET
			)
			
			if (NOT nosman_result EQUAL 0)
				nos_fatal_error("Failed to rescan modules in workspace. Please check your NOSMAN_WORKSPACE_DIR and NOSMAN_EXECUTABLE variables.")
			endif()

		nos_message(STATUS "Rescanning modules in workspace succeeded. Trying to install ${name} ${version} again.")
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

			nos_message(STATUS "Creating target ${target_name} for package ${name}-${version}")
			add_library(${target_name} INTERFACE)

			# Get module path
			string(JSON plugin_path GET "${nosman_output}" "manifest_path")
			get_filename_component(plugin_path ${plugin_path} DIRECTORY)
			cmake_path(SET plugin_path "${plugin_path}")
			# Recorded before the types are generated: generating them walks this
			# package's own dependencies, and a cycle comes back here to read it.
			set_target_properties(${target_name} PROPERTIES NOS_PACKAGE_ROOT "${plugin_path}")

			_nos_generate_package_types("${nosman_output}" package_types_target package_types_include_dirs)
			set_target_properties(${target_name} PROPERTIES
				NOS_PACKAGE_TYPES_TARGET "${package_types_target}"
				NOS_PACKAGE_TYPES_INCLUDE_DIRS "${package_types_include_dirs}")

			nos_get_files_recursive(${plugin_path} ".natvis" natvis_files)
			target_sources(${target_name} PRIVATE ${natvis_files})

			# Optional: Get "public_include_folder" from JSON output. If not found skip it
			cmake_path(SET ${target_name}_INCLUDE_DIR "${plugin_path}/Include")
			string(JSON nos_plugin_include_folder ERROR_VARIABLE err GET "${nosman_output}" "public_include_folder")
			if (err STREQUAL "NOTFOUND")
				nos_message(STATUS "Found ${name} ${version} include folder: ${nos_plugin_include_folder}")
				cmake_path(SET ${target_name}_INCLUDE_DIR "${nos_plugin_include_folder}")
			endif()
			# Generated headers come first, so that copies left in the package's
			# Include folder by an older toolchain cannot win.
			target_include_directories(${target_name} INTERFACE ${package_types_include_dirs} ${${target_name}_INCLUDE_DIR})
			set_target_properties(${target_name} PROPERTIES FOLDER "nosman")
			target_link_directories(${target_name} INTERFACE ${plugin_path}/Libraries)
		else()
			nos_fatal_error("Failed to find ${name} ${version} include folder")
		endif()
	else()
		nos_fatal_error("Unable to find nosman. Set NOSMAN_EXECUTABLE to use nos_get_package.")
	endif()
endfunction()

function(nos_get_plugin_info name version query out_var)
	nos_get_package_info(${name} ${version} ${query} ${out_var})
	set(${out_var} ${${out_var}} PARENT_SCOPE)
endfunction()

function(nos_find_plugin_path name version out_var)
	nos_find_package_path(${name} ${version} ${out_var})
	set(${out_var} ${${out_var}} PARENT_SCOPE)
endfunction()

function(nos_get_plugin name version out_target_name)
	nos_get_package(${name} ${version} ${out_target_name})
	set(${out_target_name} ${${out_target_name}} PARENT_SCOPE)
endfunction()

function(_nos_get_files_and_group folder file_types group_name out_files_var)
	nos_get_files_recursive(${folder} "${file_types}" files)
	list(LENGTH files file_count)
	if (file_count GREATER 0)
		nos_message(STATUS "Found ${file_count} files in ${folder} for group ${group_name}")
		foreach(file IN LISTS files)
			nos_message(STATUS " - ${file}")
		endforeach()
	else()
		nos_message(STATUS "No files found in ${folder} for group ${group_name}")
	endif()
	source_group("${group_name}" FILES ${files})
	set(${out_files_var} ${files} PARENT_SCOPE)
endfunction()

function(_nos_add_plugin NAME DEPENDENCIES INCLUDE_FOLDERS MANIFEST_FILE_EXT ADDITIONAL_FILE_TYPES ALTERNATIVE_MANIFEST_FILE_EXTS)
	if (NOT USE_AUTO_TARGET_GENERATION)
		nos_colored_message(COLOR CYAN "Processing plugin ${NAME}")
	endif()

	if (DEFINED NOS_PLUGIN_ROOT AND NOT NOS_PLUGIN_ROOT STREQUAL "")
		set(plugin_root "${NOS_PLUGIN_ROOT}")
	else()
		set(plugin_root "${CMAKE_CURRENT_SOURCE_DIR}")
	endif()
	set(config_folder "${plugin_root}/Config")
	set(source_folder "${plugin_root}/Source")
	set(public_include_folder "${plugin_root}/Include")
	set(shaders_folder "${plugin_root}/Shaders")

	set(NOS_PLUGIN_TYPE MODULE)
	set(NOS_LINK_PROPERTY PRIVATE)
	if (NOT EXISTS ${source_folder})
		set(NOS_PLUGIN_TYPE INTERFACE)
		set(NOS_LINK_PROPERTY INTERFACE)
	endif()

	nos_get_files_recursive(${source_folder} "${NOS_SOURCE_FILE_TYPES}" source_files)
	if (NOT source_files)
		set(NOS_PLUGIN_TYPE INTERFACE)
		set(NOS_LINK_PROPERTY INTERFACE)
	endif()
	
	nos_get_files_recursive(${public_include_folder} "${NOS_HEADER_FILE_TYPES}" header_files)

	set(config_file_types ".json")
	_nos_get_files_and_group(${config_folder} "${config_file_types}" "Config" config_files)
	_nos_get_files_and_group(${plugin_root} ".fbs" "Types" type_schema_files)
	_nos_get_files_and_group(${plugin_root} ".nosnode" "Node Definitions" node_definition_files)
	_nos_get_files_and_group(${plugin_root} ".nosdef" "Node Definitions" node_definition_files_legacy)
	_nos_get_files_and_group(${plugin_root} ".natvis" "Natvis" natvis_files)

	list(LENGTH ADDITIONAL_FILE_TYPES len_file_types_list)
	math(EXPR last_idx "${len_file_types_list} - 1")
	
	list(APPEND additional_files)
	if (last_idx GREATER 0)
		foreach(file_idx RANGE 0 ${last_idx} 2)
			unset(_files)
			math(EXPR group_idx "${file_idx} + 1")
			list(GET ADDITIONAL_FILE_TYPES ${file_idx} file_type)
			list(GET ADDITIONAL_FILE_TYPES ${group_idx} group_name)
			_nos_get_files_and_group(${plugin_root} ${file_type} ${group_name} _files)
			foreach(file IN LISTS _files)
				list(APPEND additional_files ${file})
			endforeach()
		endforeach()
	endif()

	set(shader_file_types ".glsl" ".comp" ".frag" ".vert" ".hlsl")
	nos_get_files_recursive(${plugin_root} "${shader_file_types}" shader_files)
	nos_get_files_recursive(${shaders_folder} "${shader_file_types}" shader_files)
	source_group("Shaders" FILES ${shader_files})
	set_source_files_properties(${shader_files} PROPERTIES HEADER_FILE_ONLY TRUE)

	file(GLOB PLUGIN_MANIFEST_FILE CONFIGURE_DEPENDS "${plugin_root}/*.${MANIFEST_FILE_EXT}")
	foreach (alternative_manifest_file_ext ${ALTERNATIVE_MANIFEST_FILE_EXTS})
		if (NOT alternative_manifest_file_ext STREQUAL "")
			file(GLOB ALTERNATIVE_PLUGIN_MANIFEST_FILES CONFIGURE_DEPENDS "${plugin_root}/*.${alternative_manifest_file_ext}")
			list(APPEND PLUGIN_MANIFEST_FILE ${ALTERNATIVE_PLUGIN_MANIFEST_FILES})
		endif()
	endforeach()
	set(INCLUDED_IN_PROJECT ${source_files} ${header_files} ${config_files} ${node_definition_files} ${node_definition_files_legacy} 
		${natvis_files} ${type_schema_files} ${shader_files} ${additional_files} ${PLUGIN_MANIFEST_FILE} ${ALTERNATIVE_PLUGIN_MANIFEST_FILES})
	add_library(${NAME} ${NOS_PLUGIN_TYPE} ${INCLUDED_IN_PROJECT})
	set_target_properties(${NAME} PROPERTIES
		PREFIX ""
		LIBRARY_OUTPUT_DIRECTORY "${plugin_root}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_DEBUG "${plugin_root}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELEASE "${plugin_root}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELWITHDEBINFO "${plugin_root}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_MINSIZEREL "${plugin_root}/Binaries"
	)

	if (NOT WIN32 AND NOT (NOS_PLUGIN_TYPE STREQUAL "INTERFACE"))
		# Hide internals so dyld/ld can't merge weak symbols (template
		# instantiations, Meyers-singleton guards, inline function copies)
		# across mappings when the same plugin is loaded more than once in
		# one process. `nosExportPlugin` stays visible via NOSAPI_ATTR.
		set_target_properties(${NAME} PROPERTIES
			C_VISIBILITY_PRESET hidden
			CXX_VISIBILITY_PRESET hidden
			VISIBILITY_INLINES_HIDDEN ON)
	endif()

	foreach(source IN LISTS source_files)
		get_filename_component(source_path "${source}" PATH)
		file(RELATIVE_PATH source_path_compact "${plugin_root}" "${source_path}")
		string(REPLACE "/" "\\" source_path_msvc "${source_path_compact}")
		source_group("${source_path_msvc}" FILES "${source}")
	endforeach()

	foreach(header IN LISTS header_files)
		get_filename_component(header_path "${header}" PATH)
		file(RELATIVE_PATH header_path_compact "${plugin_root}" "${header_path}")
		string(REPLACE "/" "\\" header_path_msvc "${header_path_compact}")
		source_group("${header_path_msvc}" FILES "${header}")
	endforeach()

	foreach(dependency IN LISTS DEPENDENCIES)
		# If target "dependency" type is UTILITY then add it as a dependency
		if(TARGET ${dependency})
			get_target_property(dependency_type ${dependency} TYPE)
			nos_message(STATUS "${NAME}: Adding dependency ${dependency} of type ${dependency_type}")
			if(dependency_type STREQUAL "UTILITY")
				add_dependencies(${NAME} ${dependency})
			else()
				target_link_libraries(${NAME} ${NOS_LINK_PROPERTY} ${dependency})
			endif()
		else()
			target_link_libraries(${NAME} ${NOS_LINK_PROPERTY} ${dependency})
		endif()
	endforeach()

	target_include_directories(${NAME} ${NOS_LINK_PROPERTY} ${plugin_root} ${source_folder} ${public_include_folder} ${INCLUDE_FOLDERS})

	# Produce PDBs in release mode too
	if (CMAKE_BUILD_TYPE STREQUAL "Release")
		if (MSVC AND (NOT (NOS_PLUGIN_TYPE STREQUAL "INTERFACE"))) # TODO: Remove NOS_PLUGIN_TYPE if its not needed.
			target_compile_options(${NAME} PRIVATE /Zi)
			target_link_options(${NAME} PRIVATE /DEBUG /OPT:REF /OPT:ICF)
		endif()
	endif()
endfunction()

function(nos_add_plugin NAME DEPENDENCIES INCLUDE_FOLDERS)
	_nos_add_plugin(${NAME} "${DEPENDENCIES}" "${INCLUDE_FOLDERS}" "nosplugin" "" "nossys;noscfg")
endfunction()

function(nos_add_subsystem NAME DEPENDENCIES INCLUDE_FOLDERS)
	_nos_add_plugin(${NAME} "${DEPENDENCIES}" "${INCLUDE_FOLDERS}" "nosplugin" "" "nossys")
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

function(nos_get_package_info_by_path path out_name out_version out_json)
	execute_process(
		COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" info "--manifest" ${path}
		RESULT_VARIABLE nosman_result
		OUTPUT_VARIABLE nosman_output
	)
	if(nosman_result EQUAL 0)
		string(STRIP ${nosman_output} nosman_output)
		set(err_name "")
		string(JSON package_name ERROR_VARIABLE err_name GET "${nosman_output}" info id name)
		string(JSON package_version ERROR_VARIABLE err_version GET "${nosman_output}" info id version)
		nos_message(STATUS "Package at path ${path} is ${package_name} version ${package_version}")
		
		if (err_name OR err_version)
			nos_fatal_error("Failed to get package name or version from manifest at path ${path}")
		endif()

		set(${out_name} ${package_name} PARENT_SCOPE)
		set(${out_version} ${package_version} PARENT_SCOPE)
		set(${out_json} ${nosman_output} PARENT_SCOPE)
	else()
		nos_fatal_error("Failed to find package info from path ${path}: ${nosman_output}")
	endif()
endfunction()

function(nos_normalize_plugin_name INPUT OUTPUT_VAR)
	# Step 1: split by '.'
	string(REPLACE "." ";" PARTS "${INPUT}")

	# Step 2: first item stays lowercase
	list(POP_FRONT PARTS FIRST)
	set(RESULT "${FIRST}")

	# Step 3: uppercase the first character of every subsequent part
	foreach(PART IN LISTS PARTS)
		string(SUBSTRING "${PART}" 0 1 FIRST_CHAR)
		string(SUBSTRING "${PART}" 1 -1 REMAINDER)
		string(TOUPPER "${FIRST_CHAR}" FIRST_CHAR)
		set(RESULT "${RESULT}${FIRST_CHAR}${REMAINDER}")
	endforeach()

	# Output to parent scope
	set(${OUTPUT_VAR} "${RESULT}" PARENT_SCOPE)
endfunction()

function(nos_find_immediate_plugin_dependencies json out_target_names out_target_dirs out_target_include_dirs)
	string(JSON dep_field_check ERROR_VARIABLE err GET "${json}" info dependencies)
	if (err)
		set(dep_count 0)
	else()
		# Get the number of dependency entries
		string(JSON dep_count LENGTH "${json}" info dependencies)
	endif()

	if(dep_count EQUAL 0)
		nos_message(STATUS "No dependencies found.")
		set(${out_target_names} "" PARENT_SCOPE)
		set(${out_target_dirs} "" PARENT_SCOPE)
		set(${out_target_include_dirs} "" PARENT_SCOPE)
		return()
	endif()

	# Iterate over all dependencies
	math(EXPR dep_count "${dep_count} - 1")
	foreach(i RANGE ${dep_count})
		set(found_target "")
		string(JSON dep_name GET "${json}" info dependencies ${i} name)
		string(JSON dep_version GET "${json}" info dependencies ${i} version)
		nos_message(STATUS "Finding dependency: ${dep_name} version ${dep_version}")
		nos_get_module("${dep_name}" "${dep_version}" found_target)
		nos_find_module_path("${dep_name}" "${dep_version}" found_dir)
		list(APPEND _deps "${found_target}")
		list(APPEND _dep_dirs "${found_dir}")

		# Build the dependency's type headers before anything that includes them,
		# and find them ahead of any copy left in its Include folder.
		get_target_property(dep_types_target ${found_target} NOS_PACKAGE_TYPES_TARGET)
		if (dep_types_target)
			list(APPEND _deps "${dep_types_target}")
		endif()
		get_target_property(dep_types_include_dirs ${found_target} NOS_PACKAGE_TYPES_INCLUDE_DIRS)
		if (dep_types_include_dirs)
			list(APPEND _dep_include_dirs ${dep_types_include_dirs})
		endif()

		nos_normalize_plugin_name(${dep_name} target_name)
		list(APPEND _dep_include_dirs "${found_dir}/Include/${target_name}")
	endforeach()
	
	set(${out_target_names} "${_deps}" PARENT_SCOPE)
	set(${out_target_dirs} "${_dep_dirs}" PARENT_SCOPE)
	set(${out_target_include_dirs} "${_dep_include_dirs}" PARENT_SCOPE)
endfunction()

# Deprecated, use _plugin functions instead.
function(nos_get_module_info name version query out_var)
	nos_get_package_info(${name} ${version} ${query} ${out_var})
	set(${out_var} ${${out_var}} PARENT_SCOPE)
endfunction()

function(nos_find_module_path name version out_var)
	nos_find_package_path(${name} ${version} ${out_var})
	set(${out_var} ${${out_var}} PARENT_SCOPE)
endfunction()

function(nos_get_module name version out_target_name)
	nos_get_package(${name} ${version} ${out_target_name})
	set(${out_target_name} ${${out_target_name}} PARENT_SCOPE)
endfunction()

function(nos_get_short_vendor_name plugin_name out_vendor_name)
	# Split by dot
	string(REPLACE "." ";" parts "${plugin_name}")

	# Get first namespace
	list(GET parts 0 ns)

	# Return
	set(${out_vendor_name} "${ns}" PARENT_SCOPE)
endfunction()
