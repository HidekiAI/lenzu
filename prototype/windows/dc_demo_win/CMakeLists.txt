# CMakeList.txt : CMake project for dc_demo_win, include source and define
# project specific logic here.
#
cmake_minimum_required (VERSION 3.8)

set(CMAKE_TOOLCHAIN_FILE C:/Users/HidekiAI/projects/github/vcpkg/scripts/buildsystems/vcpkg.cmake)


# Enable Hot Reload for MSVC compilers if supported.
if (POLICY CMP0141)
  cmake_policy(SET CMP0141 NEW)
  set(CMAKE_MSVC_DEBUG_INFORMATION_FORMAT "$<IF:$<AND:$<C_COMPILER_ID:MSVC>,$<CXX_COMPILER_ID:MSVC>>,$<$<CONFIG:Debug,RelWithDebInfo>:EditAndContinue>,$<$<CONFIG:Debug,RelWithDebInfo>:ProgramDatabase>>")
endif()


project ("dc_demo_win")
find_package(OpenCV REQUIRED)

# Add source to this project's executable.
include_directories(${OpenCV_INCLUDE_DIRS})
add_executable (dc_demo_win "dc_demo_win.cpp" "dc_demo_win.h")
target_link_libraries(dc_demo_win ${OpenCV_LIBS})

if (CMAKE_VERSION VERSION_GREATER 3.12)
  set_property(TARGET dc_demo_win PROPERTY CXX_STANDARD 20)
endif()


# TODO: Add tests and install targets if needed.

