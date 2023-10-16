SET _BUILD_DIR=.\\build
SET _VCPKG_DIR=\\usr\\src\\github\\vcpkg

%_VCPKG_DIR%\\vcpkg install opencv
cmake -B %_BUILD_DIR% -S . -DCMAKE_TOOLCHAIN_FILE=%_VCPKG_DIR%\\scripts\\buildsystems\\vcpkg.cmake
cmake --build %_BUILD_DIR%

