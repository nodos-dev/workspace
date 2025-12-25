# Nodos CMake Utilities
#
# This file contains utility functions for the Nodos CMake build system.
#
# Verbose Mode:
# By default, informational STATUS messages are suppressed to reduce clutter.
# To enable verbose output, use either:
#   cmake --log-level=VERBOSE ...
#   cmake -DNOS_VERBOSE=ON ...
#
# Supported colors: NORMAL, BLACK, RED, GREEN, YELLOW, BLUE,                   #
#                   MAGENTA, CYAN, WHITE                                       #
# Supported style:  BOLD, DIMMED                                               #
#                                                                              #
# (C) 2024 Marc Schöndorf                                                      #
# Licensed under the zlib License. See LICENSE.md                              #
# Changes:                                                                     #
#  - Modified for Nodos SDK CMake Tools, function names are changed            #
function(_nos_color_format_text)
    cmake_parse_arguments(PARSE_ARGV 0 "_TEXT" "BOLD;DIMMED" "COLOR" "")
    
    # ANSI color codes (faster than execute_process)
    # ESC character using octal escape (CMake doesn't support \x hex escapes)
    string(ASCII 27 _ESC)
    set(_RESET "${_ESC}[0m")
    
    # Build ANSI code list
    set(_ANSI_CODES "")
    
    # Add style attributes
    if(_TEXT_BOLD)
        list(APPEND _ANSI_CODES "1")
    endif()
    
    if(_TEXT_DIMMED)
        list(APPEND _ANSI_CODES "2")
    endif()
    
    # Add color attribute
    if(_TEXT_COLOR)
        string(TOLOWER "${_TEXT_COLOR}" _TEXT_COLOR_LOWER)
        
        # Map color names to ANSI codes
        if(_TEXT_COLOR_LOWER STREQUAL "normal")
            set(_COLOR_NUM "0")
        elseif(_TEXT_COLOR_LOWER STREQUAL "black")
            set(_COLOR_NUM "30")
        elseif(_TEXT_COLOR_LOWER STREQUAL "red")
            set(_COLOR_NUM "31")
        elseif(_TEXT_COLOR_LOWER STREQUAL "green")
            set(_COLOR_NUM "32")
        elseif(_TEXT_COLOR_LOWER STREQUAL "yellow")
            set(_COLOR_NUM "33")
        elseif(_TEXT_COLOR_LOWER STREQUAL "blue")
            set(_COLOR_NUM "34")
        elseif(_TEXT_COLOR_LOWER STREQUAL "magenta")
            set(_COLOR_NUM "35")
        elseif(_TEXT_COLOR_LOWER STREQUAL "cyan")
            set(_COLOR_NUM "36")
        elseif(_TEXT_COLOR_LOWER STREQUAL "white")
            set(_COLOR_NUM "37")
        endif()
        
        if(DEFINED _COLOR_NUM)
            list(APPEND _ANSI_CODES "${_COLOR_NUM}")
        endif()
    endif()
    
    # Combine ANSI codes with semicolons
    set(_COLOR_CODE "")
    if(_ANSI_CODES)
        list(JOIN _ANSI_CODES ";" _COMBINED_CODES)
        set(_COLOR_CODE "${_ESC}[${_COMBINED_CODES}m")
    endif()
    
    # Format text with ANSI codes
    set(_FORMATTED_TEXT_RESULT "${_COLOR_CODE}[Nodos] ${_TEXT_UNPARSED_ARGUMENTS}${_RESET}")
    
    # Save result into COLOR_FORMATTED_TEXT for parent scope access
    set(COLOR_FORMATTED_TEXT ${_FORMATTED_TEXT_RESULT} PARENT_SCOPE)
endfunction()

# Formats given string with colors and appends the result to
# the COLOR_FORMATTED_TEXT_COMBINED variable, which can be used
# in the parent scope.
#
# Example:  _nos_color_format_text_append(COLOR BLUE "My blue text")
#           _nos_color_format_text_append(BOLD COLOR RED "My bold red text")
#           _nos_color_format_text_append(DIMMED "My dimmed text")
#
# To print: message("${COLOR_FORMATTED_TEXT_COMBINED}")
#
# (C) 2024 Marc Schöndorf
# Licensed under the zlib License.
# Changes:
#  - Modified for Nodos SDK CMake Tools, function names are changed
function(_nos_color_format_text_append)
    _nos_color_format_text(${ARGN})
    
    # Append formatted text to COLOR_FORMATTED_TEXT_COMBINED
    set(COLOR_FORMATTED_TEXT_COMBINED "${COLOR_FORMATTED_TEXT_COMBINED}${COLOR_FORMATTED_TEXT}" PARENT_SCOPE)
endfunction()

# Directly prints formatted text
#
# Example:  nos_colored_message(COLOR BLUE "My blue text")
#           nos_colored_message(BOLD COLOR RED "My bold red text")
#           nos_colored_message(DIMMED "My dimmed text")
# 
# (C) 2024 Marc Schöndorf
# Licensed under the zlib License.
# Changes:
#  - Modified for Nodos SDK CMake Tools, function names are changed
function(nos_colored_message)
    _nos_color_format_text(${ARGN})
    message(${COLOR_FORMATTED_TEXT})
endfunction()

function(nos_fatal_error)
	_nos_color_format_text(BOLD COLOR RED ${ARGN})
	message(FATAL_ERROR ${COLOR_FORMATTED_TEXT})
endfunction()

# Prints a message only in verbose mode, otherwise suppresses it
# Use this for informational messages that don't need to clutter the output
# Respects both --log-level=VERBOSE and -DNOS_VERBOSE=ON
function(nos_message)
	cmake_language(GET_MESSAGE_LOG_LEVEL current_log_level)
	if(current_log_level MATCHES "VERBOSE|DEBUG|TRACE" OR NOS_VERBOSE)
		_nos_color_format_text(${ARGN})
		message(${COLOR_FORMATTED_TEXT})
	endif()
endfunction()

function(nos_get_sanitized_engine_folder_name nos_sdk_dir out)
    # Extract folder name from SDK directory path and sanitize for CMake target names
    get_filename_component(engine_folder_name "${nos_sdk_dir}" DIRECTORY)
    get_filename_component(engine_folder_name "${engine_folder_name}" NAME)
    # Replace all non-alphanumeric characters with underscores, then clean up
    string(REGEX REPLACE "[^a-zA-Z0-9]" "_" sanitized_engine_folder_name "${engine_folder_name}")
    string(REGEX REPLACE "_+" "_" sanitized_engine_folder_name "${sanitized_engine_folder_name}")
    string(REGEX REPLACE "^_|_$" "" sanitized_engine_folder_name "${sanitized_engine_folder_name}")

    set(${out} "${sanitized_engine_folder_name}" PARENT_SCOPE)
endfunction()

function(_nos_generate_sdk_target_name sdk_type sdk_version nos_sdk_dir out)
    # Generate target name in format: ${sdk_type}_${version_suffix}__${sanitized_engine_folder_name}
    # Args:
    #   sdk_type: "nosPluginSDK" or "nosSubsystemSDK"
    #   sdk_version: version string like "1.2.3" 
    #   nos_sdk_dir: SDK directory path
    #   out: output variable name
    
    nos_get_sanitized_engine_folder_name(${nos_sdk_dir} sanitized_engine_folder_name)
    string(REPLACE "." "_" version_target_suffix "${sdk_version}")
    set(${out} "${sdk_type}_${version_target_suffix}__${sanitized_engine_folder_name}" PARENT_SCOPE)
endfunction()

function(_nos_generate_nodos_target_name target_type nodos_version nos_sdk_dir out)
    # Generate target name in format: ${target_type}_${version_suffix}__${sanitized_engine_folder_name}
    # Args:
    #   target_type: "nosLauncher" or "nosEditor"
    #   nodos_version: nodos version string like "1.2.3" 
    #   nos_sdk_dir: SDK directory path
    #   out: output variable name
    
    nos_get_sanitized_engine_folder_name(${nos_sdk_dir} sanitized_engine_folder_name)
    string(REPLACE "." "_" version_target_suffix "${nodos_version}")
    set(${out} "${target_type}_${version_target_suffix}__${sanitized_engine_folder_name}" PARENT_SCOPE)
endfunction()