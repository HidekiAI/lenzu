#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <string>
#include <vector>

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
#ifdef _WIN32
    // Windows-specific paths
    auto itaijidictpath = "..\\..\\assets\\itaijidict";
    auto kanwadictpath = "..\\..\\assets\\kanwadict";
#else
    // Linux-specific paths
    auto itaijidictpath = "/usr/share/kakasi/itaijidict";
    auto kanwadictpath = "/usr/share/kakasi/kanwadict";
#endif

    // Command-line arguments for kakasi - kakasi_getopt_argv() will verify/check if dicts exist (if passed)
    //std::vector<std::string> argv = {"-JH", "-f", "-i", "utf-8", "-o", "utf-8", itaijidictpath, kanwadictpath}; // NOTE: kakasi accepts both "utf8" and "utf-8", but does not like "UTF-8"
    std::vector<std::string> argv = {"-JH", "-f", "-o", "utf-8", itaijidictpath, kanwadictpath}; // NOTE: currently, -i utf-8 causes kakasi_do() to hang IF at least one char is Japanese (actually, any \escaped hex as well).  If it was all ASCII, it won't crash with '-i utf8'
    // cannot do shared_ptr<char> or unique_ptr<char> as kakasi_getopt_argv() expects char ** (C-style array of char pointers)
    char **argv_c = new char *[argv.size()];    // yes, I'm explicitly declaring type rather than auto here as a reminder that I've allocated memory...
    for (int i = 0; i < argv.size(); i++)
    {
        std::cout << "argv[" << i << "]: " << argv[i] << std::endl;
        argv_c[i] = new char[128];
        strncpy_s(argv_c[i], 128, argv[i].c_str(), argv[i].length());
    }
    // NOTE: kakasi_getopt_argv() will verify/check if dicts exist (if passed)
    kakasi_getopt_argv(argv.size(), argv_c);

    // kakasi -JH -f -i utf-8 -o utf-8 path_to_dict1 path_to_dict2 <<< "最近人気の デスクトップな リナックスです!"
    auto text_utf8 = std::string("最近人気の デスクトップな リナックスです!");
    std::cout << "kakasi_do(" << text_utf8 << ") strlen(" << strlen(text_utf8.c_str()) << ") - length=" << text_utf8.length() << " bytes" << std::endl;
    auto converted_string_buffer = kakasi_do((char *)text_utf8.c_str()); // c_str() will return a pointer to a null-terminated string (C code always expects null-terminators)
    std::cout << "Result: '" << converted_string_buffer << "'" << std::endl;

    // Clean up
    std::cout << "cleaning up string buffer/pool..." << std::endl;
    kakasi_free(converted_string_buffer);
    kakasi_close_kanwadict();
    delete[] argv_c; // probably should delete[] each element of argv_c as well but this is a simple example

    // Clean up and unload the library (Windows-specific)
#ifdef USE_DLL
#ifdef _WIN32
    FreeLibrary(hKakasiDLL);
#endif
#endif
    return 0;
}
