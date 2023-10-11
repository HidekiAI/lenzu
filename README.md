# lenzu
desktop lens (almost like desktop magnifier commonly found in accessibility application for the visually impaired users) which detects images (initially started as OCR) to real-time analyze (via OpenCV) the small window where the mouse cursor hovers (for example, for OCR, it can then add furigana to all the kanji)

## dependencies:
* Google Cloud:
  * Google Oauth2
  * Google Vision
* OpenCV
* Rust opencv

Currently under experimentation in which OpenCV may not be able to screen capture desktops in Linux, also on my tripple-screen, need to prototype on which screen my mouse cursor is located to, before I can crop that screen around the location where the mouse cursor is positioned.  And lastly, I only can do tripple-screen on my Windows laptop, and have not verified it on Linux (Debian).
