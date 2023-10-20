# CMakeList.txt : CMake project for lenzu, include source and define
# project specific logic here.
#
cmake_minimum_required (VERSION 3.10)
project ("lenzu")

# Find the Rust library
find_library(RUST_LIBRARY my_rust_library PATHS ${CMAKE_SOURCE_DIR}/target/release)

# Create a C++ executable that links with the Rust library
add_executable(lenzu src/main.cpp)
target_link_libraries(lenzu ${RUST_LIBRARY})

# Set the C++ standard for the lenzu target
if (CMAKE_VERSION VERSION_GREATER 3.12)
  set_property(TARGET lenzu PROPERTY CXX_STANDARD 20)
endif()

# Enable Hot Reload for MSVC compilers if supported.
if (POLICY CMP0141)
  cmake_policy(SET CMP0141 NEW)
  set(CMAKE_MSVC_DEBUG_INFORMATION_FORMAT "$<IF:$<AND:$<C_COMPILER_ID:MSVC>,$<CXX_COMPILER_ID:MSVC>>,$<$<CONFIG:Debug,RelWithDebInfo>:EditAndContinue>,$<$<CONFIG:Debug,RelWithDebInfo>:ProgramDatabase>>")
endif()

# Add source to this project's executable.
#add_executable (lenzu "src/*.cpp" "src/*.h" "src/*.hpp")

# TODO: Add tests and install targets if needed.
