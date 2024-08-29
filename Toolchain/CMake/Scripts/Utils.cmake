# Supported colors: NORMAL, BLACK, RED, GREEN, YELLOW, BLUE,                   #
#                   MAGENTA, CYAN, WHITE                                       #
# Supported style:  BOLD                                                       #
#                                                                              #
# (C) 2024 Marc Schöndorf                                                      #
# Licensed under the zlib License. See LICENSE.md                              #
# Changes:                                                                     #
#  - Modified for Nodos SDK CMake Tools, function names are changed            #
function(__nos_color_format_text)
    cmake_parse_arguments(PARSE_ARGV 0 "_TEXT" "BOLD" "COLOR" "")
    set(_FORMAT_OPTIONS -E cmake_echo_color --no-newline)
    
    # Do we have a color attribute?
    if(_TEXT_COLOR)
        string(TOLOWER "${_TEXT_COLOR}" _TEXT_COLOR_LOWER)
        
        # Is it a valid color attribute?
        if(${_TEXT_COLOR_LOWER} MATCHES "^normal|black|red|green|yellow|blue|magenta|cyan|white")
            list(APPEND _FORMAT_OPTIONS --${_TEXT_COLOR_LOWER})
        endif()
    endif()
    
    # Do we have a BOLD attribute?
    if(_TEXT_BOLD)
        list(APPEND _FORMAT_OPTIONS --bold)
    endif()
    
    # Run CMake command to format text and write result to _FORMATTED_TEXT_RESULT
    execute_process(COMMAND ${CMAKE_COMMAND} -E env CLICOLOR_FORCE=1 ${CMAKE_COMMAND} ${_FORMAT_OPTIONS} ${_TEXT_UNPARSED_ARGUMENTS}
                    OUTPUT_VARIABLE _FORMATTED_TEXT_RESULT
                    ECHO_ERROR_VARIABLE)
    
    # Save result into COLOR_FORMATTED_TEXT for parent scope access
    set(COLOR_FORMATTED_TEXT ${_FORMATTED_TEXT_RESULT} PARENT_SCOPE)
endfunction()

# Formats given string with colors and appends the result to
# the COLOR_FORMATTED_TEXT_COMBINED variable, which can be used
# in the parent scope.
#
# Example:  __nos_color_format_text_append(COLOR BLUE "My blue text")
#           __nos_color_format_text_append(BOLD COLOR RED "My bold red text")
#
# To print: message("${COLOR_FORMATTED_TEXT_COMBINED}")
#
# (C) 2024 Marc Schöndorf
# Licensed under the zlib License.
# Changes:
#  - Modified for Nodos SDK CMake Tools, function names are changed
function(__nos_color_format_text_append)
    __nos_color_format_text(${ARGN})
    
    # Append formatted text to COLOR_FORMATTED_TEXT_COMBINED
    set(COLOR_FORMATTED_TEXT_COMBINED "${COLOR_FORMATTED_TEXT_COMBINED}${COLOR_FORMATTED_TEXT}" PARENT_SCOPE)
endfunction()

# Directly prints formatted text
#
# Example:  nos_colored_message(COLOR BLUE "My blue text")
#           nos_colored_message(BOLD COLOR RED "My bold red text")
# 
# (C) 2024 Marc Schöndorf
# Licensed under the zlib License.
# Changes:
#  - Modified for Nodos SDK CMake Tools, function names are changed
function(nos_colored_message)
    __nos_color_format_text(${ARGN})
    message(${COLOR_FORMATTED_TEXT})
endfunction()