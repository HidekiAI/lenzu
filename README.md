# lenzu

Desktop lens (almost like desktop magnifier commonly found in accessibility application for the visually impaired users) which detects images (initially started as OCR) to real-time analyze (via tesseract) the small window where the mouse cursor hovers, it can then overlay translations on top of the detected text.

Initial intentions (and probably will be my default settings) is to use it for Japanese (JP) OCR and only overlay hiragana over the kanji like a furigana.  Possibly, later mod can make it even like rikai-kun/rikai-chan.  But it should be able to overlay anything, like:

- Overlay romaji over any and all Japanese
- Link to Google Translate (or maybe even DeepL) and overlay the localization text.  IMHO, Azure has the most richest number of translation, but Google Translator is better than DeepL (DeepL tends to try to do slangs, in which the translation gets lost a lot more than what I think the author intended - well, at least for Japanese, I felt that way).
  - I do not know how Rikai-chan/kun worked but I think it had its own local dictionary/jisho, so ideally, this idea can also be achieved with no online dependencies.

Overall, it's basic idea is to have it do Google Lens (only the OCR part) on your desktop (Linux and Windows) without having it connect to the cloud.  It should be able to do it offline when/where possible.

I am not a GUI expert, so the app will be of minimal "it works as intended but no more, no less" and be all configured via command line args :grin:

## Dependencies

- [tesseract](https://github.com/tesseract-ocr/tesseract) - Probably should the THE library for OCR hands-down
  - I'm opting for tesseract with [JP tessdata](https://github.com/tesseract-ocr/tessdata/blob/main/jpn.traineddata) (also [JP vertical](https://github.com/tesseract-ocr/tessdata/blob/main/jpn_vert.traineddata)) so offline (no dependencies to the cloud) is possible.
- [kakasi](http://kakasi.namazu.org/index.html.ja) - Japanese morphological analyzer.  I'm using this to get the furigana for the kanji.  At least for [Debian](https://manpages.debian.org/testing/kakasi/kakasi.1.ja.html), it's a [lib](https://packages.debian.org/en/stable/libs/libkakasi2) so I can probably rust-bindgen it or use the existing [rust-kakasi](https://crates.io/crates/kakasi) which I've used it once, in which I cannot remember at the moment but  I had some issues on UTF-8 and JIS not getting translated, and ended up just directly using CLI kakasi instead.  In any case, kakasi or no way at all :grin:
- [xcap](https://github.com/nashaofu/xcap) - screen capture library, at the time of writing, this is still maintined and has the capabilities to capture from multi-monitor!
- [winit](https://github.com/rust-windowing/winit) - Possibly abandon this library and write my own for Linux and Windows.  I'm mostly interested in 3 things:
  - Mouse event - there seems to be no cross-platform library at the moment, in which even if the window/app is in focus, when mouse is hovering over desktop outside the monitor/screen where the app resides on, it does not register the mouse coordinates correctly
  - Canvas - simply, a window (with or without borders, I do not care) where I can render the furigana overlayed on the text where the mouse is hovering over
  - Keyboard event - Similar to mouse event, I need to be able to listen to keyboard events, so that I can close the app when I press the ESC key (only when lenzu-app window is in focus).
  - All in all, at the moment, I only have Windows so it's one of those "I'll use 3rd party libraries that has been tested on XYZ operating system and assume it works as long as it works on my ABC operating system."

Currently under experimentation in which OpenCV may not be able to screen capture desktops in Linux, also on my tripple-screen, need to prototype on which screen my mouse cursor is located to, before I can crop that screen around the location where the mouse cursor is positioned.  And lastly, I only can do tripple-screen on my Windows laptop, and have not verified it on Linux (Debian).  (P/S: I really hate Windows11 WDM sluggishness as well as memory hogging WSL2, I want my Debian back!)


## Techinal Design

Well, to be honest, it's so straight forward, that it does not need to have one, so I'll just bullet point list in order of sequences:

1. Open your application such as web-browser or manga-reader or even Kindle
1. Open the lenzu-app on the monitor (in case of multi-monitors) where you want to capture text.  You can either use the command line argument to tell it which monitor, or just create it on any, then drag it to the same screen as your "other" app.
   - This (at the moment it seems, though I think it is possible on any monitor as seen with Windows Magnifier) is because the mouse cursor seems to only tell you position relative to each screen (well, at least the libs I've evaluated was on that state).
   - Note that xcap (for Linux and Windows) can capture multi-monitor
   - you want to make sure lenzu-app does not obstruct as much visual with your "other app"
1. Use alt-tab to switch between the "other" app and lenzu-app, and once you see a word (i.e. a kanji) on your "other" app that needs to be overlayed (i.e. furigana, romaji, etc) alt-tab back to lenzu-app and hover your mouse over the text.
1. Capture rectangle where the mouse cursor is, and keep updating lenzu-app canvas in realitime so that users can visually see what lenzu is seeing.
1. Once you're over the text you want to interact with, hit space-key.  The space (or whatever trigger) key is needed, because trying to dynamically (realitime) translate text over everything it captures is just going to cause hug lag.
1. The captured image will then be evaluated by tesseract
1. Whatever text that tesseract recognizes will then be evaluated by kakasi to get the furigana
1. The furigana will then be rendered into texture-buffer as an overlay, opaque (no alpha) boxed on text (so that when overlayed, it will just obstruct what's behind/underneath)
1. The original image will then be layered as background, and then the furigana will be rendered on top of it.
1. Keep the image on the canvas until esc key is pressed, in which, it can then start to capture rectangle in realitime until space-key trigger is hit again.
