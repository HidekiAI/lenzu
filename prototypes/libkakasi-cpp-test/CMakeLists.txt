cmake_minimum_required(VERSION 3.25.1)
project(libkakasi_cpp_test)

# Add the executable target
add_executable(kakasi_cpp_test_exec src/main.cpp)

# Link the Kakasi library

# Set library paths based on the operating system
if (CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(CMAKE_LIBRARY_PATH "${CMAKE_LIBRARY_PATH}/lib;/usr/lib/kakasi;${CMAKE_LIBRARY_PATH}/lib/x86_64-linux-gnu/;")

    #explicitly link
    target_link_libraries(kakasi_cpp_test_exec PRIVATE ${CMAKE_SOURCE_DIR}/lib/x86_64-linux-gnu/libkakasi.a)

    # "ld -rpth"
    target_link_directories(kakasi_cpp_test_exec PRIVATE ${CMAKE_SOURCE_DIR}/lib/x86_64-linux-gnu/)

    # force "-L"
    target_link_options(kakasi_cpp_test_exec PRIVATE -Wl,--no-as-needed,-L${CMAKE_SOURCE_DIR}/lib/x86_64-linux-gnu/)
elseif (CMAKE_SYSTEM_NAME STREQUAL "Windows")
    set(CMAKE_LIBRARY_PATH "${CMAKE_LIBRARY_PATH}/lib;/usr/lib/kakasi;${CMAKE_LIBRARY_PATH}/lib/x86_64-mingw/;")

    #explicitly link
    target_link_libraries(kakasi_cpp_test_exec PRIVATE ${CMAKE_SOURCE_DIR}/lib/x86_64-mingw-gnu/libkakasi.a)

    # "ld -rpth"
    target_link_directories(kakasi_cpp_test_exec PRIVATE ${CMAKE_SOURCE_DIR}/lib/x86_64-mingw/)
    # force "-L"
    target_link_options(kakasi_cpp_test_exec PRIVATE -Wl,--no-as-needed,-L${CMAKE_SOURCE_DIR}/lib/x86_64-mingw/)
else()
    message(FATAL_ERROR "Unsupported operating system: ${CMAKE_SYSTEM_NAME}")
endif()

set(CMAKE_EXE_LINKER_FLAGS "-v --verbose")

# usage of `-l:libfoo.a` (basically, the ":") will bias search for static library over dll/so shared
target_link_libraries(kakasi_cpp_test_exec PRIVATE :libkakasi.a)

