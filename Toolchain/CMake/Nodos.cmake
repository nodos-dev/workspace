macro(nos_find_sdk version out_nos_plugin_sdk out_nos_subsystem_sdk out_sdk_dir)
	# Search for that specific version. If not found, warn the user about using the latest compatible version.
	list(FIND NOS_VERSIONS ${version} version_index)

	if(version_index EQUAL -1)
		message(WARNING "Nodos version ${version} not found. Using the latest compatible version.")
		set(max_compatible_version -1)

		foreach(index RANGE 1 ${NOS_SDK_END_RANGE})
			list(GET NOS_VERSIONS ${index} nos_version)
			string(REPLACE "." ";" nos_version_components ${nos_version})
			list(GET nos_version_components 0 major)
			list(GET nos_version_components 1 minor)

			if(major EQUAL version)
				if(minor GREATER max_compatible_version)
					set(max_compatible_version ${NOS_MINOR})
					set(version_index ${index})
				endif()
			endif()
		endforeach()

		if(index EQUAL -1)
			message(FATAL_ERROR "No compatible version found.")
		else()
			list(GET NOS_VERSIONS ${version_index} nos_version)
			list(GET NOS_SDK_DIRS ${version_index} nos_sdk_dir)
			set(${out_sdk_dir} ${nos_sdk_dir})
			string(REPLACE "." "_" version_target_suffix ${nos_version})
			set(${out_nos_plugin_sdk} nosPluginSDK_${version_target_suffix})
			set(${out_nos_subsystem_sdk} nosSubsystemSDK_${version_target_suffix})
		endif()
	endif()

	message(STATUS "Using Nodos version ${version}")
	list(GET NOS_VERSIONS ${version_index} nos_version)
	list(GET NOS_SDK_DIRS ${version_index} nos_sdk_dir)
	set(${out_sdk_dir} ${nos_sdk_dir})
	string(REPLACE "." "_" version_target_suffix ${nos_version})
	set(${out_nos_plugin_sdk} nosPluginSDK_${version_target_suffix})
	set(${out_nos_subsystem_sdk} nosSubsystemSDK_${version_target_suffix})
endmacro()


function(nos_make_plugin_project NAME DEPENDENCIES INCLUDE_FOLDERS)
	project(${NAME})
	message("Processing plugin ${NAME}")

	set(SOURCE_FOLDER "${CMAKE_CURRENT_SOURCE_DIR}/Source")
	set(CONFIG_FOLDERS "${CMAKE_CURRENT_SOURCE_DIR}" "${CMAKE_CURRENT_SOURCE_DIR}/Config")

	file(GLOB_RECURSE SOURCES CONFIGURE_DEPENDS ${SOURCE_FOLDER}
		"${SOURCE_FOLDER}/*.cpp" "${SOURCE_FOLDER}/*.inl" "${SOURCE_FOLDER}/*.glsl" "${SOURCE_FOLDER}/*.hlsl"
		"${SOURCE_FOLDER}/*.comp" "${SOURCE_FOLDER}/*.frag" "${SOURCE_FOLDER}/*.vert"
		"${SOURCE_FOLDER}/*.py")
	list(APPEND CONFIG_FILES)

	foreach(CONFIG_FOLDER ${CONFIG_FOLDERS})
		file(GLOB_RECURSE CUR_CONFIG_FILES CONFIGURE_DEPENDS ${CONFIG_FOLDER}
			"${CONFIG_FOLDER}/*.noscfg" "${CONFIG_FOLDER}/*.nosdef" "${CONFIG_FOLDER}/*.fbs")
		list(APPEND CONFIG_FILES ${CUR_CONFIG_FILES})
	endforeach()

	file(GLOB_RECURSE HEADERS CONFIGURE_DEPENDS ${SOURCE_FOLDER} "${SOURCE_FOLDER}/*.h" "${SOURCE_FOLDER}/*.hpp")
	file(GLOB_RECURSE RESOURCES CONFIGURE_DEPENDS ${SOURCE_FOLDER} "${SOURCE_FOLDER}/*.rc")

	set(SHADER_FOLDERS "${CMAKE_CURRENT_SOURCE_DIR}" "${CMAKE_CURRENT_SOURCE_DIR}/Shaders")
	list(APPEND SHADERS)

	foreach(SHADER_FOLDER ${SHADER_FOLDERS})
		file(GLOB_RECURSE CUR_SHADERS CONFIGURE_DEPENDS ${SHADER_FOLDER}
			"${SHADER_FOLDER}/*.glsl" "${SHADER_FOLDER}/*.comp" "${SHADER_FOLDER}/*.frag" "${SHADER_FOLDER}/*.vert")
		list(APPEND SHADERS ${CUR_SHADERS})
	endforeach()

	add_library(${NAME} MODULE ${SOURCES} ${SHADERS} ${HEADERS} ${RESOURCES} ${CONFIG_FILES})
	set_target_properties(${NAME} PROPERTIES
		CXX_STANDARD 20
		LIBRARY_OUTPUT_DIRECTORY_DEBUG "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELEASE "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELWITHDEBINFO "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_MINSIZEREL "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
	)

	foreach(source IN LISTS SOURCES)
		get_filename_component(source_path "${source}" PATH)
		string(REPLACE "${CMAKE_CURRENT_SOURCE_DIR}" "" source_path_compact "${source_path}")
		string(REPLACE "/" "\\" source_path_msvc "${source_path_compact}")
		source_group("${source_path_msvc}" FILES "${source}")
	endforeach()

	foreach(header IN LISTS HEADERS)
		get_filename_component(header_path "${header}" PATH)
		string(REPLACE "${CMAKE_CURRENT_SOURCE_DIR}" "" header_path_compact "${header_path}")
		string(REPLACE "/" "\\" header_path_msvc "${header_path_compact}")
		source_group("${header_path_msvc}" FILES "${header}")
	endforeach()

	target_include_directories(${NAME} PRIVATE ${INCLUDE_FOLDERS})
	target_link_libraries(${NAME} PRIVATE ${DEPENDENCIES})
endfunction()

function(nos_make_subsystem_project NAME DEPENDENCIES INCLUDE_FOLDERS)
	project(${NAME})
	message("Processing subsystem ${NAME}")

	set(SOURCE_FOLDERS "${CMAKE_CURRENT_SOURCE_DIR}/Source" "${CMAKE_CURRENT_SOURCE_DIR}/Include")
	set(CONFIG_FOLDERS "${CMAKE_CURRENT_SOURCE_DIR}" "${CMAKE_CURRENT_SOURCE_DIR}/Config")

	foreach(folder IN LISTS SOURCE_FOLDERS)
		message(STATUS "${PROJECT_NAME}: Scanning ${folder}")
		file(GLOB_RECURSE SOURCES CONFIGURE_DEPENDS ${folder} "${folder}/*.cpp"
			"${folder}/*.cc" "${folder}/*.c" "${folder}/*.inl"
			"${folder}/*.frag" "${folder}/*.vert" "${folder}/*.glsl" "${folder}/*.comp" "${folder}/*.dat" "${folder}/*.natvis" "${folder}/*.py")
		file(GLOB_RECURSE HEADERS CONFIGURE_DEPENDS ${folder} "${folder}/*.h" "${folder}/*.hpp")
		file(GLOB_RECURSE RESOURCES CONFIGURE_DEPENDS ${folder} "${folder}/*.rc")
		list(APPEND COLLECTED_SOURCES ${SOURCES})
		list(APPEND COLLECTED_HEADERS ${HEADERS})
		list(APPEND COLLECTED_RESOURCES ${RESOURCES})
	endforeach()

	foreach(CONFIG_FOLDER ${CONFIG_FOLDERS})
		file(GLOB_RECURSE CUR_CONFIG_FILES CONFIGURE_DEPENDS ${CONFIG_FOLDER}
			"${CONFIG_FOLDER}/*.nossys" "${CONFIG_FOLDER}/*.fbs" "${CONFIG_FOLDER}/Defaults.json")
		list(APPEND CONFIG_FILES ${CUR_CONFIG_FILES})
	endforeach()

	add_library(${NAME} MODULE ${COLLECTED_SOURCES} ${COLLECTED_HEADERS} ${COLLECTED_RESOURCES} ${CONFIG_FILES})

	set_target_properties(${NAME} PROPERTIES
		CXX_STANDARD 20
		LIBRARY_OUTPUT_DIRECTORY_DEBUG "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELEASE "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_RELWITHDEBINFO "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
		LIBRARY_OUTPUT_DIRECTORY_MINSIZEREL "${CMAKE_CURRENT_SOURCE_DIR}/Binaries"
	)

	foreach(source IN LISTS COLLECTED_SOURCES)
		get_filename_component(source_path "${source}" PATH)
		string(REPLACE "${CMAKE_CURRENT_SOURCE_DIR}" "" source_path_compact "${source_path}")
		string(REPLACE "/" "\\" source_path_msvc "${source_path_compact}")
		source_group("${source_path_msvc}" FILES "${source}")
	endforeach()

	foreach(header IN LISTS COLLECTED_HEADERS)
		get_filename_component(header_path "${header}" PATH)
		string(REPLACE "${CMAKE_CURRENT_SOURCE_DIR}" "" header_path_compact "${header_path}")
		string(REPLACE "/" "\\" header_path_msvc "${header_path_compact}")
		source_group("${header_path_msvc}" FILES "${header}")
	endforeach()

	foreach(resource IN LISTS COLLECTED_RESOURCES)
		get_filename_component(resource_path "${resource}" PATH)
		string(REPLACE "${CMAKE_CURRENT_SOURCE_DIR}" "" resource_path_compact "${resource_path}")
		string(REPLACE "/" "\\" header_path_msvc "${resource_path_compact}")
		source_group("${resource_path_msvc}" FILES "${resource}")
	endforeach()

	target_include_directories(${NAME} PRIVATE ${INCLUDE_FOLDERS} ${SOURCE_FOLDERS})
	target_link_libraries(${NAME} PRIVATE ${DEPENDENCIES})
endfunction()

function(nos_get_module name version out_target_name)
    if (NOT DEFINED NODOS_WORKSPACE_DIR)
        message(FATAL_ERROR "NODOS_WORKSPACE_DIR is not defined. Set it to the path of the workspace where modules will be installed.")
    endif()
	message(STATUS "Searching for Nodos module ${name} ${version} in workspace")
	if(NOSMAN_EXECUTABLE)
		execute_process(
			COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NODOS_WORKSPACE_DIR}" info ${name} ${version} "include_folder"
			RESULT_VARIABLE NOSMAN_RESULT
			OUTPUT_VARIABLE NOSMAN_OUTPUT
		)

		string(REPLACE "." "_" target_name ${name})
		string(REPLACE "." "_" version_str ${version})
		string(APPEND target_name "-v${version_str}")
		string(PREPEND target_name "__nos_generated__")
		set(${out_target_name} ${target_name} PARENT_SCOPE)

		if(NOSMAN_RESULT EQUAL 0)
			string(STRIP ${NOSMAN_OUTPUT} NOSMAN_OUTPUT)
			cmake_path(SET ${target_name}_INCLUDE_DIR "${NOSMAN_OUTPUT}")
			message(STATUS "Found ${name} ${version} include folder: ${${target_name}_INCLUDE_DIR}")
			if (TARGET ${target_name})
				message(STATUS "Module ${name}-${version} found in project. Using existing target.")
				return()
			endif()
			add_library(${target_name} INTERFACE)
			file(GLOB_RECURSE include_files "${NOSMAN_OUTPUT}/*")
			target_sources(${target_name} PUBLIC ${include_files})
			target_include_directories(${target_name} INTERFACE ${${target_name}_INCLUDE_DIR})
		else()
			message(FATAL_ERROR "Failed to find ${name} ${version} include folder")
		endif()
	else()
		message(FATAL_ERROR "Unable to find nosman. Set NOSMAN_EXECUTABLE to use nos_get_module.")
	endif()
endfunction()