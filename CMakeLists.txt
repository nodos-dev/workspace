cmake_minimum_required(VERSION 3.24.2)

project(NodosWorkspace)

set(NOS_VERSIONS "")
set(NOS_SDK_DIRS "")

set(ENGINE_FOLDER ${CMAKE_CURRENT_SOURCE_DIR}/Engine)

file(GLOB ENGINE_DIRS "${ENGINE_FOLDER}/*")

set(WITH_NODOS_WORKSPACE ON)

foreach(ENGINE_DIR ${ENGINE_DIRS})
    if(IS_DIRECTORY ${ENGINE_DIR})
        add_subdirectory(${ENGINE_DIR})
    endif()
endforeach()

foreach(NOS_VERSION ${NOS_VERSIONS})
    message(STATUS "NOS_VERSION: ${NOS_VERSION}")
endforeach()

foreach(NOS_SDK_DIR ${NOS_SDK_DIRS})
    message(STATUS "NOS_SDK_DIR: ${NOS_SDK_DIR}")
endforeach()

# Prepare targets
list(LENGTH NOS_VERSIONS NOS_SDK_COUNT)
math(EXPR NOS_SDK_END_RANGE "${NOS_SDK_COUNT} - 1")
message(STATUS "Found ${NOS_SDK_END_RANGE} Nodos SDKs")
foreach(val RANGE 1 ${NOS_SDK_END_RANGE})
    list(GET NOS_VERSIONS ${val} nos_version)
    list(GET NOS_SDK_DIRS ${val} nos_sdk_dir)

    message(STATUS "(${val}/${NOS_SDK_END_RANGE}) Creating targets for Nodos version ${nos_version} at ${nos_sdk_dir}")
    
    # Convert version 1.2.3 to 1_2_3
    string(REPLACE "." "_" version_target_suffix "${nos_version}")
    
    # Create nosPluginSDK target
    add_library(nosPluginSDK_${version_target_suffix} INTERFACE)
    target_compile_definitions(nosPluginSDK_${version_target_suffix} INTERFACE "-DNOMINMAX")
    target_include_directories(nosPluginSDK_${version_target_suffix} INTERFACE ${nos_sdk_dir}/include)

    # Create nosSubsystemSDK target
    add_library(nosSubsystemSDK_${version_target_suffix} INTERFACE)
    target_compile_definitions(nosSubsystemSDK_${version_target_suffix} INTERFACE "-DNOMINMAX")
    target_include_directories(nosSubsystemSDK_${version_target_suffix} INTERFACE ${nos_sdk_dir}/include)
    
    # Exclude from all
    set_target_properties(nosPluginSDK_${version_target_suffix} PROPERTIES EXCLUDE_FROM_ALL TRUE)
    set_target_properties(nosSubsystemSDK_${version_target_suffix} PROPERTIES EXCLUDE_FROM_ALL TRUE)
endforeach()

macro(find_nodos version out_nos_plugin_sdk out_nos_subsystem_sdk)
    # Search for that specific version. If not found, warn the user about using the latest compatible version.
    list(FIND NOS_VERSIONS ${version} version_index)
    if(version_index EQUAL -1)
        message(WARNING "Nodos version ${version} not found. Using the latest compatible version.")
        set(max_compatible_version -1)
        set(out_version_index -1)
        foreach(index RANGE 1 ${NOS_SDK_END_RANGE})
            list(GET NOS_VERSIONS ${index} nos_version)
            string(REPLACE "." ";" nos_version_components ${nos_version})
            list(GET nos_version_components 0 major)
            list(GET nos_version_components 1 minor)
            if(major EQUAL version)
                if(minor GREATER max_compatible_version)
                    set(max_compatible_version ${NOS_MINOR})
                    set(out_version_index ${index})
                endif()
            endif()
        endforeach()
        if(index EQUAL -1)
            message(FATAL_ERROR "No compatible version found.")
        else()
            list(GET NOS_VERSIONS ${out_version_index} nos_version)
            string(REPLACE "." "_" version_target_suffix ${nos_version})
            set(${out_nos_plugin_sdk} nosPluginSDK_${version_target_suffix})
            set(${out_nos_subsystem_sdk} nosSubsystemSDK_${version_target_suffix})
        endif()
    endif()
    message(STATUS "Using Nodos version ${version}")
    list(GET NOS_VERSIONS ${version_index} nos_version)
    string(REPLACE "." "_" version_target_suffix ${nos_version})
    set(${out_nos_plugin_sdk} nosPluginSDK_${version_target_suffix})
    set(${out_nos_subsystem_sdk} nosSubsystemSDK_${version_target_suffix})
endmacro()

function(add_folder_as_plugin_project NAME DEPENDENCIES INCLUDE_FOLDERS)
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

function(add_folder_as_subsystem_project NAME DEPENDENCIES INCLUDE_FOLDERS)
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

	if(TARGET ${NAME})
		target_sources(${NAME} PRIVATE ${COLLECTED_SOURCES} ${COLLECTED_HEADERS} ${COLLECTED_RESOURCES} ${CONFIG_FILES})
	else()
		add_library(${NAME} MODULE ${COLLECTED_SOURCES} ${COLLECTED_HEADERS} ${COLLECTED_RESOURCES} ${CONFIG_FILES})
	endif()

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

if (NOT DEFINED MODULE_DIRS OR MODULE_DIRS STREQUAL "")
	message(STATUS "Scanning for modules in ${CMAKE_CURRENT_SOURCE_DIR}/Module")
	set(MODULE_DIRS "" CACHE INTERNAL "Module directories" FORCE)

	function(collect_first_cmake_directories dir)
		file(GLOB SUBDIRS RELATIVE ${dir} ${dir}/*)
		foreach(subdir ${SUBDIRS})
			if (IS_DIRECTORY ${dir}/${subdir})
				if (EXISTS "${dir}/${subdir}/CMakeLists.txt")
					set(MODULE_DIRS ${MODULE_DIRS} ${dir}/${subdir} CACHE INTERNAL "Module directories" FORCE)
					message(STATUS "Found module directory: ${dir}/${subdir}")
				else()
					collect_first_cmake_directories(${dir}/${subdir})
				endif()
			endif()
		endforeach()
	endfunction()

	collect_first_cmake_directories("${CMAKE_CURRENT_SOURCE_DIR}/Module")
	message(STATUS "Found ${MODULE_DIRS} module paths")
endif()

foreach(MODULE_DIR ${MODULE_DIRS})
	if (IS_DIRECTORY ${MODULE_DIR})
		message(STATUS "Processing module directory: ${MODULE_DIR}")
		add_subdirectory(${MODULE_DIR})
	endif()
endforeach()