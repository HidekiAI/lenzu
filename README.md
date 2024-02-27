# lenzu

desktop lens (almost like desktop magnifier commonly found in accessibility application for the visually impaired users) which detects images (initially started as OCR) to real-time analyze (via OpenCV) the small window where the mouse cursor hovers (for example, for OCR, it can then add furigana to all the kanji)

## dependencies

* Google Cloud:
  * Google Oauth2
  * Google Vision
* OpenCV
* Rust opencv

Currently under experimentation in which OpenCV may not be able to screen capture desktops in Linux, also on my tripple-screen, need to prototype on which screen my mouse cursor is located to, before I can crop that screen around the location where the mouse cursor is positioned.  And lastly, I only can do tripple-screen on my Windows laptop, and have not verified it on Linux (Debian).

## Build/compile noets

Mainly pains involved in Windows (not Linux/Debian)...

Assuming you're using MSYS64 (MingGW) in which you can install packages via
`pacman`, you want to make sure some of the packages are installed specific to
the target, or else libs such as `leptonica` won't build at all.  I really hate
Windows and I really wish I can just stay/stick with Linux...

```bash
#
$ pacman -S --needed base-devel mingw-w64-i686-cmake mingw-w64-i686-toolchain mingw-w64-i686-ninja
```

And all in all, you'll need to do this:

```bash
#$ export LEPTONICA_INCLUDE_PATH=/c/msys64/clang64/include/leptonica/ export LEPTONICA_LINK_PATHS=/c/msys64/clang64/lib/ ; export LEPTONICA_LINK_LIBS="leptonica" ; export TESSERACT_INCLUDE_PATHS=/c/msys64/clang64/include/tesseract/ ; export TESSERACT_LINK_PATHS=/c/msys64/clang64/lib ; export TESSERACT_LINK_LIBS="tesseract"

$ export LEPTONICA_INCLUDE_PATH=/c/msys64/clang64/include/leptonica/ 
$ export LEPTONICA_LINK_PATHS=/c/msys64/clang64/lib/ 
$ export LEPTONICA_LINK_LIBS="leptonica" 
$ export TESSERACT_INCLUDE_PATHS=/c/msys64/clang64/include/tesseract/ 
$ export TESSERACT_LINK_PATHS=/c/msys64/clang64/lib 
$ export TESSERACT_LINK_LIBS="tesseract"
```

Note that above example assumes that you have BASH installed in `C:\MSYS64\` directory.
