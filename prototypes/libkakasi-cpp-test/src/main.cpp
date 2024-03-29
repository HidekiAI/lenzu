#include <cstdio>
#include <cstdlib>

// Include Windows-specific headers only if compiling on Windows
#ifdef _WIN32
#include <windows.h>
#endif

// Include libkakasi header

extern "C"
{
#include "libkakasi.h"
}

int main()
{
#ifdef USE_DLL
#ifdef _WIN32
    // Load libkakasi dynamically (Windows-specific)
    HMODULE hKakasiDLL = LoadLibrary("C:\\kakasi\\bin");
    int(__cdecl * kakasi_getopt_argv)(int, char **) = (int(__cdecl *)(int, char **))GetProcAddress(hKakasiDLL, "kakasi_getopt_argv");
    char *(__cdecl * kakasi_do)(const char *) = (char *(__cdecl *)(const char *))GetProcAddress(hKakasiDLL, "kakasi_do");
    int(__cdecl * kakasi_free)(char *p) = (int(__cdecl *)(char *p))GetProcAddress(hKakasiDLL, "kakasi_free");
#endif
#endif
    // Set dictionary paths (platform-independent)
    char *itaijidictpath = nullptr;
    char *kanwadictpath = nullptr;

#ifdef _WIN32
    // Windows-specific paths
    itaijidictpath = "..\\..\\assets\\itaijidict";
    kanwadictpath = "..\\..\\assets\\kanwadict";
#else
    // Linux-specific paths
    itaijidictpath = "/usr/share/kakasi/itaijidict";
    kanwadictpath = "/usr/share/kakasi/kanwadict";
#endif

    // Set environment variables
    // putenv(("ITAIJIDICTPATH=" + std::string(itaijidictpath)).c_str());
    // putenv(("KANWADICTPATH=" + std::string(kanwadictpath)).c_str());

    // Command-line arguments for kakasi
    char *argv[] = {"kakasi", "-JH", "-f", "-i", "utf-8", "-o", "utf-8", itaijidictpath, kanwadictpath};
    kakasi_getopt_argv(3, argv);

    // kakasi -JH -f -i utf-8 -o utf-8 path_to_dict1 path_to_dict2 <<< "最近人気の デスクトップな リナックスです!"
    char *converted_string_buffer = kakasi_do("最近人気の デスクトップな リナックスです!");
    printf("%s\n", converted_string_buffer);

    // Clean up
    kakasi_free(converted_string_buffer);

    // Clean up and unload the library (Windows-specific)
#ifdef USE_DLL
#ifdef _WIN32
    FreeLibrary(hKakasiDLL);
#endif
#endif
    return 0;
}
