# Copyright MediaZ Teknoloji A.S. All Rights Reserved.
macro(nos_find_sdk requested_version out_nos_plugin_sdk out_nos_subsystem_sdk out_sdk_dir)
	# Search for the requested version
	list(FIND NOS_VERSIONS ${requested_version} found_version_idx)

	if(found_version_idx EQUAL -1)
		# Requested version not found, find the latest compatible version
		set(max_compatible_minor -1)
		set(max_compatible_patch -1)

		string(REPLACE "." ";" req_ver_components ${requested_version})
		list(GET req_ver_components 0 req_major)
		list(GET req_ver_components 1 req_minor)
		list(GET req_ver_components 2 req_patch)

		# First, look for the exact requested minor version
		foreach(index RANGE 1 ${NOS_SDK_END_RANGE})
			list(GET NOS_VERSIONS ${index} found_version)
			string(REPLACE "." ";" nos_version_components ${found_version})
			list(GET nos_version_components 0 major)
			list(GET nos_version_components 1 minor)
			list(GET nos_version_components 2 patch)

			if(major EQUAL req_major AND minor EQUAL req_minor)
				if(patch GREATER_EQUAL req_patch)
					set(max_compatible_minor ${minor})
					set(max_compatible_patch ${patch})
					set(found_version_idx ${index})
					break()
				endif()
			endif()
		endforeach()

		# If the exact minor version is not found, look for the latest compatible minor version
		if(found_version_idx EQUAL -1)
			foreach(index RANGE 1 ${NOS_SDK_END_RANGE})
				list(GET NOS_VERSIONS ${index} found_version)
				string(REPLACE "." ";" nos_version_components ${found_version})
				list(GET nos_version_components 0 major)
				list(GET nos_version_components 1 minor)
				list(GET nos_version_components 2 patch)

				if(major EQUAL req_major)
					if(minor GREATER max_compatible_minor OR (minor EQUAL req_minor AND patch GREATER_EQUAL req_patch))
						set(max_compatible_minor ${minor})
						set(max_compatible_patch ${patch})
						set(found_version_idx ${index})
					endif()
				endif()
			endforeach()
		endif()

		if(found_version_idx EQUAL -1)
			nos_fatal_error("No compatible version found for requested SDK version ${requested_version}.")
		endif()
	endif()

	list(GET NOS_VERSIONS ${found_version_idx} found_version)
	list(GET NOS_SDK_DIRS ${found_version_idx} nos_sdk_dir)
	list(GET NOS_PLUGIN_SDK_VERSIONS ${found_version_idx} found_plugin_sdk_version)
	list(GET NOS_SUBSYSTEM_SDK_VERSIONS ${found_version_idx} found_subsystem_sdk_version)
	nos_message(STATUS "Using Nodos version ${found_version}")
	_nos_generate_sdk_target_name("nosPluginSDK" "${found_plugin_sdk_version}" "${nos_sdk_dir}" plugin_sdk_target_name)
	set(${out_nos_plugin_sdk} ${plugin_sdk_target_name})
	if (found_subsystem_sdk_version)
		_nos_generate_sdk_target_name("nosSubsystemSDK" "${found_subsystem_sdk_version}" "${nos_sdk_dir}" subsystem_sdk_target_name)
		set(${out_nos_subsystem_sdk} ${subsystem_sdk_target_name})
	else()
		set(${out_nos_subsystem_sdk} ${${out_nos_plugin_sdk}})
	endif()
	
	if (${found_version} VERSION_GREATER_EQUAL "1.4.0")
		set(nos_sdk_dir ${nos_sdk_dir}/Plugin)
		set(FLATC_EXECUTABLE "${nos_sdk_dir}/Binaries/flatc" CACHE PATH "Path to the flatc executable" FORCE)
		set(NOS_SDK_TYPES_DIR "${nos_sdk_dir}/Types" CACHE PATH "Path to the Nodos SDK types directory" FORCE)
	else()
		set(nos_sdk_dir ${nos_sdk_dir})
		set(FLATC_EXECUTABLE "${nos_sdk_dir}/bin/flatc" CACHE PATH "Path to the flatc executable" FORCE)
		set(NOS_SDK_TYPES_DIR "${nos_sdk_dir}/types" CACHE PATH "Path to the Nodos SDK types directory" FORCE)
	endif()
	set(${out_sdk_dir} ${nos_sdk_dir})
endmacro()

macro(nos_find_plugin_sdk requested_sdk_version out_sdk_target out_sdk_dir)
	if (${requested_sdk_version} VERSION_GREATER_EQUAL "39.11.0")
		nos_get_package("nodos.sdk.plugin" ${requested_sdk_version} sdk_target_name)
		if (NOT TARGET ${sdk_target_name})
			nos_fatal_error("Failed to find Nodos Plugin SDK ${requested_sdk_version}.")
		endif()
		nos_get_package_info("nodos.sdk.plugin" ${requested_sdk_version} "manifest_path" sdk_manifest_path)
		get_filename_component(sdk_path "${sdk_manifest_path}" DIRECTORY)
		set(${out_sdk_target} ${sdk_target_name})
		set(${out_sdk_dir} ${sdk_path})
	
		set(FLATC_EXECUTABLE "${sdk_path}/Binaries/flatc" CACHE PATH "Path to the flatc executable" FORCE)
	else()
		# For all nodos sdks, call `nodos sdk-info ${requested_sdk_version} plugin` and read the json if ret code is 0
		if(NOT NOSMAN_EXECUTABLE)
			nos_fatal_error("Unable to find nosman. Set NOSMAN_EXECUTABLE to use nos_find_plugin_sdk.")
		endif()
		execute_process(
			COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" sdk-info ${requested_sdk_version} plugin
			RESULT_VARIABLE result_code
			OUTPUT_VARIABLE sdk_info_json
		)
		if (NOT result_code EQUAL 0)
			nos_fatal_error("Unable to find compatible Plugin SDK version for requested version ${requested_sdk_version}.")
		endif()

		# Parse the JSON output to extract the SDK directory
		string(JSON plugin_sdk_version  ERROR_VARIABLE err GET "${sdk_info_json}" "version")
		if (NOT err STREQUAL "NOTFOUND")
			message(FATAL_ERROR "Unable to parse JSON output: ${err}")
		endif()
		string(JSON sdk_path  ERROR_VARIABLE err GET "${sdk_info_json}" "path")
		if (NOT err STREQUAL "NOTFOUND")
			message(FATAL_ERROR "Unable to parse JSON output: ${err}")
		endif()

		set(plugin_sdk_path ${sdk_path})

		nos_message(STATUS "Using Nodos Plugin SDK version ${plugin_sdk_version}")

		set(root_nodos_sdk_folder ${sdk_path})
		if (${plugin_sdk_version} VERSION_GREATER_EQUAL "39.11.0")
			# Get the parent folder of root_nodos_sdk_folder
			get_filename_component(root_nodos_sdk_folder "${root_nodos_sdk_folder}" DIRECTORY)
		endif()

		_nos_generate_sdk_target_name("nosPluginSDK" "${plugin_sdk_version}" "${root_nodos_sdk_folder}" plugin_sdk_target_name)
		set(${out_sdk_target} ${plugin_sdk_target_name})
		set(${out_sdk_dir} ${plugin_sdk_path})

		if (${plugin_sdk_version} VERSION_GREATER_EQUAL "39.11.0")
			set(FLATC_EXECUTABLE "${plugin_sdk_path}/Binaries/flatc" CACHE PATH "Path to the flatc executable" FORCE)
		else()
			set(FLATC_EXECUTABLE "${plugin_sdk_path}/bin/flatc" CACHE PATH "Path to the flatc executable" FORCE)
		endif()
	endif()
endmacro()