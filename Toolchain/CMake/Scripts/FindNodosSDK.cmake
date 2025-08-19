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
			nos_fatal_error("No compatible version found for requested version ${requested_version}.")
		endif()
		list(GET NOS_VERSIONS ${found_version_idx} found_version)
		list(GET NOS_SDK_DIRS ${found_version_idx} nos_sdk_dir)
		set(${out_sdk_dir} ${nos_sdk_dir})
		string(REPLACE "." "_" version_target_suffix ${found_version})
		set(${out_nos_plugin_sdk} nosPluginSDK_${version_target_suffix})

		if (found_version VERSION_GREATER_EQUAL "1.4.0")
			set(${out_nos_subsystem_sdk} ${out_nos_plugin_sdk})
		else()
			set(${out_nos_subsystem_sdk} nosSubsystemSDK_${version_target_suffix})
		endif()
	endif()

	list(GET NOS_VERSIONS ${found_version_idx} found_version)
	list(GET NOS_SDK_DIRS ${found_version_idx} nos_sdk_dir)
	list(GET NOS_PLUGIN_SDK_VERSIONS ${found_version_idx} found_plugin_sdk_version)
	list(GET NOS_SUBSYSTEM_SDK_VERSIONS ${found_version_idx} found_subsystem_sdk_version)
	message(STATUS "Using Nodos version ${found_version}")
	set(${out_sdk_dir} ${nos_sdk_dir})
	string(REPLACE "." "_" plugin_sdk_version_target_suffix "${found_plugin_sdk_version}")
	set(${out_nos_plugin_sdk} nosPluginSDK_${plugin_sdk_version_target_suffix})
	if (found_subsystem_sdk_version)
		string(REPLACE "." "_" subsystem_sdk_target_suffix "${found_subsystem_sdk_version}")
		set(${out_nos_subsystem_sdk} nosSubsystemSDK_${subsystem_sdk_target_suffix})
	else()
		set(${out_nos_subsystem_sdk} ${out_nos_plugin_sdk})
	endif()
endmacro()

macro(nos_find_plugin_sdk requested_plugin_sdk_version out_nos_plugin_sdk out_sdk_dir)
# For all nodos sdks, call `nodos sdk-info ${requested_plugin_sdk_version} plugin` and read the json if ret code is 0
	if(NOT NOSMAN_EXECUTABLE)
		nos_fatal_error("Unable to find nosman. Set NOSMAN_EXECUTABLE to use nos_find_plugin_sdk.")
	endif()
	execute_process(
		COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NOSMAN_WORKSPACE_DIR}" sdk-info ${requested_plugin_sdk_version} plugin
		RESULT_VARIABLE result_code
		OUTPUT_VARIABLE sdk_info_json
		ERROR_QUIET
	)
	if (NOT result_code EQUAL 0)
		nos_fatal_error("Unable to find compatible Plugin SDK version for requested version ${requested_plugin_sdk_version}.")
	endif()
	# Parse the JSON output to extract the SDK directory
	string(JSON sdk_plugin_version  ERROR_VARIABLE err GET "${sdk_info_json}" "version")
	if (NOT err STREQUAL "NOTFOUND")
		message(FATAL_ERROR "Unable to parse JSON output: ${err}")
	endif()
	string(JSON sdk_path  ERROR_VARIABLE err GET "${sdk_info_json}" "path")
	if (NOT err STREQUAL "NOTFOUND")
		message(FATAL_ERROR "Unable to parse JSON output: ${err}")
	endif()

	message(STATUS "Using Nodos Plugin SDK version ${sdk_plugin_version}")

	string(REPLACE "." "_" sdk_plugin_version_target_suffix "${sdk_plugin_version}")

	set(${out_nos_plugin_sdk} nosPluginSDK_${sdk_plugin_version_target_suffix})
	set(${out_sdk_dir} ${sdk_path})

endmacro()