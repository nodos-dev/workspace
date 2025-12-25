# Copyright MediaZ Teknoloji A.S. All Rights Reserved.

set(NOS_MODULE_DIR_INDEX 0)
set(ALL_MODULE_DIRS "" CACHE INTERNAL "All module directories" FORCE)

function(collect_first_cmake_directories dir out_dirs)
	file(GLOB SUBDIRS RELATIVE ${dir} ${dir}/*)

	if(EXISTS "${dir}/CMakeLists.txt")
		set(${out_dirs} ${${out_dirs}} ${dir} CACHE INTERNAL "Module directories" FORCE)
		nos_message("Found module directory: ${dir}/${subdir}")
	else()
		foreach(subdir ${SUBDIRS})
			if(IS_DIRECTORY ${dir}/${subdir})
				collect_first_cmake_directories(${dir}/${subdir} ${out_dirs})
			endif()
		endforeach()
	endif()
endfunction()

foreach(cur_module_dir ${MODULE_DIRS})
	# If relative, should be relative to NODOS_WORKSPACE_DIR
	if(NOT IS_ABSOLUTE ${cur_module_dir})
		set(cur_module_dir "${NODOS_WORKSPACE_DIR}/${cur_module_dir}")
	endif()
	nos_colored_message(COLOR GREEN "Scanning for modules in ${cur_module_dir}")
	collect_first_cmake_directories("${cur_module_dir}" ALL_MODULE_DIRS)
endforeach()

foreach(dir ${ALL_MODULE_DIRS})
	if(IS_DIRECTORY ${dir})
		nos_colored_message(COLOR GREEN "Processing module directory: ${dir}")
		add_subdirectory(${dir} "${CMAKE_CURRENT_BINARY_DIR}/ModuleDir${NOS_MODULE_DIR_INDEX}")
		math(EXPR NOS_MODULE_DIR_INDEX "${NOS_MODULE_DIR_INDEX} + 1")
	endif()
endforeach()