function(nos_get_module name version out_target_name)
    if(NOT DEFINED NODOS_WORKSPACE_DIR)
        message(FATAL_ERROR "NODOS_WORKSPACE_DIR is not defined. Set it to the path of the workspace where modules will be installed.")
    endif()

    message(STATUS "Searching for Nodos module ${name} ${version} in workspace")

    # TODO: Download if not exists.
    if(NOSMAN_EXECUTABLE)
        # Install module if not exists, silently
        execute_process(
            COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NODOS_WORKSPACE_DIR}" install ${name} ${version}
            RESULT_VARIABLE NOSMAN_RESULT
            OUTPUT_QUIET
        )
        if (NOT NOSMAN_RESULT EQUAL 0)
            message(FATAL_ERROR "Failed to install ${name} ${version} in workspace")
        endif()

        execute_process(
            COMMAND ${NOSMAN_EXECUTABLE} --workspace "${NODOS_WORKSPACE_DIR}" info ${name} ${version} --relaxed
            RESULT_VARIABLE NOSMAN_RESULT
            OUTPUT_VARIABLE NOSMAN_OUTPUT
        )

        if (NOSMAN_RESULT EQUAL 0)
            string(REPLACE "." "_" target_name ${name})
            string(REPLACE "." "_" version_str ${version})
            string(APPEND target_name "-v${version_str}")
            string(PREPEND target_name "__nos_generated__")

            string(STRIP ${NOSMAN_OUTPUT} NOSMAN_OUTPUT)
            # Get "public_include_folder" from JSON output
            string(JSON nos_module_include_folder GET "${NOSMAN_OUTPUT}" "public_include_folder")
            cmake_path(SET ${target_name}_INCLUDE_DIR "${nos_module_include_folder}")
            message(STATUS "Found ${name} ${version} include folder: ${${target_name}_INCLUDE_DIR}")

            set(${out_target_name} ${target_name} PARENT_SCOPE)

            if(TARGET ${target_name})
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