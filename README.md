# lenzu

Desktop lens (almost like desktop magnifier commonly found in accessibility application for the visually impaired users) which detects images (initially started as OCR) to real-time analyze (possibly via OpenCV) the small window where the mouse cursor hovers (for example, for OCR, it can then add furigana to all the kanji via kakasi or analyze for phonetics via mecab for possibly screen-reading).

## Libraries

Target platforms are Linux (Debian) and Windows (MinGW64).  Not really relevant to list it here since all you need to do is look at my Cargo.toml but just in case:

- kakasi (I believe rust version is self-contained, so no need to install executable version)
- tesseract (rusty-tesseract expects tesseract-ocr executable and it's trained-data pre-installed)
- leptonica (the MinGW pacman version statically links leptonica it seems, so you won't find MinGW libs for this one, you'll have to hand-compile using MinGW gcc)
- windows-rs (features: Media_Ocr, Globalization) - also want to make sure to install (in Windows Settings) for Japanese language
- winit

## Tesseract versus Windows OCR

Firstly, I want to emphasize that performance means nothing, if accuracies is just crap...  With that said, currently for offline OCR, I've tried

- [Tesseract](https://github.com/tesseract-ocr/tesseract)
- [Windows Media.OCR](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr)
- [Manga-OCR](https://github.com/kha-white/manga-ocr) (Note that his work `mokuro` which integrates manga-ocr is just superior!)

Note that though I've installed EasyOCR, but it turns out it does NOT handle vertical Japanese, hence it's not even part of the considerations.  I've also tried to run [gazou](https://github.com/kamui-fin/gazou) but if you look in the "Dependencies" section, it clearly states it requires tesseract (and leptonica), so I've skipped on that.  Incidentally, installing leptonica (even via MinGW) is just painfully time-consuming on Windows (it's really a breeze on Linux).

With that said, I've found that Manga-OCR is the most accurate, but the problem it turns out, is that the folks who publishes the [Manga109s dataset](http://www.manga109.org/ja/index.html) are not too responsive and never got back to me, so I have to toss this out of my toolbox as well.

    NOTE: As of 2024-03-05 (password expired in 5 days), I've finally got a response from them, and have acquired UID+password to download the data (3.1GB) they've used for training.  Hence I may attempt to retrain "jpn_vert" for tesseract-traineddata using their batch in the future (if/when I have time).

So I'm now left with tesseract and Windows Media.Ocr...  First, the test I've used is the CC licensed manga [Ubunchu!](http://www.aerialline.com/comics/ubunchu/):

![Ubunchu Manga](assets/ubunchu01_02.png)

Here's the text converted result for Microsoft.Media.Ocr:

    ```text
    $ time cargo run --bin sample_ocr media/ubunchu01_02.png
        Finished dev [unoptimized + debuginfo] target(s) in 0.55s
        Running `target\debug\sample_ocr.exe media/ubunchu01_02.png`
    Windows: Evaluating '"media/ubunchu01_02.png"' for forward-slashes
    驫 朝 轗 あ た し の オ ス ス メ は う ぶ ん ち ゅ / 衫 ク 最 近 人 気 の デ ス ク ト ッ プ な リ ナ ッ ク ス で す , 、 ubuntu ※ う ぶ ん ゅ く っ 0 ン 物 ィ 夘 し な い マ 夛 い て ん ー ん 化 秘 ー 一 瞬 く ら い 検 討 し て ′ 、 だ さ い よ ー

    real    0m1.856s  <--- ~2 seconds
    user    0m0.000s
    sys     0m0.000s
    ```

Note: See comments below in regards to "くださいよー!" being correct on one pattern but incorrect (above/here) for this usage.

And the following is for tesseract:

    ```text
    $ time tesseract.exe media/ubunchu01_02.png stdout -l jpn+jpn_vert+osd --psm 6
    =    N    2    。 465
    、 いい       必メ - の        い
    スッの          7  <        ち
    すな \ン。
    -   2_/   AN ubuntu
    NOか
    N       と\        王i       @
    ※ うぶんあみゅではなくりプッヒゥで3          |
    て      \ 一  ーー レブ ーベデーー ー ェー
    ミミの
    ee
    上 っ    ツア王SN 2にンンジSS
    だ討瞬    を(タジン| の
    シンニーテデい
    | にあこ
        レイ人 ーー シク

    real    0m33.681s   <- ~34 sec
    user    0m0.032s
    sys     0m0.000s
    ```

And with manga-ocr via mokuro:

    ```text
    # time mokuro  --disable_confirmation=True /tmp/mangadir/ ; ls -lAh
    Paths to process:
    C:\msys64\tmp\mangadir
    2024-03-06 16:19:49.269 | INFO     | mokuro.run:run:48 - Processing 1/1: C:\msys64\tmp\mangadir
    Processing pages...: 100%|███████████████████ ... ███████████████| 1/1 [00:00<00:00, 10.03it/s]
    2024-03-06 16:19:49.433 | INFO     | mokuro.run:run:56 - Processed successfully: 1/1

    real    0m32.217s   <--- ~32 sec (but "processing pages" itself only took like 300 mSec!)
    user    0m0.000s
    sys     0m0.000s
    total 296K
    ```

I had to use mokuro because manga-ocr kept on crashing...  But the "Processing pages" part looks like only took ~300 mSec...  mokuro does extra stuffs like generate html with text position where you can hover over the text to get the text from.  But it's a great app (it was hell compiling it though because the pip version did not work)

Overall, I think manga-ocr is probably the most ideal candidate IF it works, but it's just too finiky and I think I spent more time trying to install it more than code/test it.

As for tesseract, I've tried ALL PSM settings, and PSM==5 was the closest it go to some recognition, hence I really wanted to get a hold of that manga109s data to train tesseract, but they're not responding, so I've given up.

Lastly, the most attractive (ease of library usage, documentations, free/access, accuracies, performance, dependabilities, linking against libs, etc) Microsoft.Media.Ocr is just the most ideal choice!  I cannot stop praising  Microsoft!  But unfortunately, this library restricts to just Windows.

Somebody on reddit mentioned that Apple also has descent accuracies but you'd have to download the language seprately and it is proprietary;  These are commonly due to the commercial operating system companies wanting to make sure the target country has language support.  For Windows for example, if you want Japanese support in which your installation was from non-Japanese installer version, you will have to have the desktop settings system download the Japanese language supprts from Microsoft (see for example [TryCreateFromUserProfileLanguages()](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine.trycreatefromuserprofilelanguages) method) in which if the target [Language](https://learn.microsoft.com/en-us/uwp/api/windows.globalization.language) is not installed, it will not be able to OCR successfully.  And as mentioned, unfortunately, it is propritary to the operating systems.  And in general, these are somewhat tied mainly IMHO (this is just an opinion) because it needs to support accessibilities for screen-readers (TTS) for vision-impared.

I'm sure Linux Desktop has TTS accessibilities which does screen-reader, but I've never been able to successfully get [mecab](https://taku910.github.io/mecab/) or Orca (yes, I like Gnome) to work well in harmony.  There are some excellent web-based TTS which even will flavor the voice according to your choices, but that's actually non-OCR topic, mainly because these are post-OCR-processed applications (they expect actual TEXT, after image has been optically recognized and converted to text).  And lastly (on this topic outside OCD) most screen-readers for vision-impared do NOT OCR (read images), they usally only read texts (UTF-8, JIS, etc), which is a different topic (see mecab, and other libraries which will analyze neighboring texts and determine how to pronounce it (phonetically) - for my purpose, I use [kakasi](http://kakasi.namazu.org/index.html.ja) which does neighbor analysis based on dictionary/jisho (basically, jisho already have "words" of 2 or more sequential kanji in pronounciation via hiragana); but enough on non-OCR topic...

In the end, for now, I've given up on other platforms and concentrating strictly on Windows using Microsoft's Windows.Microsoft.Media.Ocr library, since I just want offline OCR (that's the key, "offline OCR").

Side note: I don't know how they do it, but the new "Snipping Tool" available on Windows 10/11 has this feature called "Text Action" in which I presume is using the same library, but for some reason, it takes longer time (I think it took like 7 seconds) and here's the result:

    ```text
    リナックスです!
    /1モリなか”ら
    デスクトップな
    最近人気の
    オススメは
    BU!
    ※うぶんちゅではなくウイントカです
    あ
    うぶんちゆ
    あたしの
    ubuntu
    却下!
    Tっわしない
    マミ112元
    んだぞ!(
    よけんなー
    このっl
    くださいよー!
    検討して
    一瞬くらい
    ```

Unfortunately, the tool has no interface/options to instruct that it should OCR from top-to-bottom+right-to-left so the order of the text becomes left-to-right (backwards) even though it figures out it is vertical text (top-to-bottom).  In any case, it's not a OCR application, but has the capabilities and I'm sure it's using the same library.

But what I wanted to point out is (though I'm still convinced they both use the same library) the hiragana 'く' on this one (the last phrase "くださいよー!") is correct, as compared to the direct usage of the windows.Media.Ocr library, the 'く' turns into '′ ' (upper part of 'く') and '、' (lower part of 'く') which looks like hiragana 'く'.

Some claim that on certain usages of OCR, the `゛` (as in `ぶ`) and `゜` (as in `ぷ`) sometimes gets smeared on low-quality images and cannot be read.  There are 2 kinds of [Google Lens](https://lens.google.com/), the web interface version and Android Application version.  At least on Android Application version, probably because the camera will force it to capture at high resolution, I've never seen this phenomenom occur.

Incidentally, the web-version of Google Lens OCR'd as:

    ```text
    最近人気の
    デスクトップな
    リナックスです!
    あ
    あたしの
    オススメは
    うぶんちゅ
    ubuntu
    ※うぶんちゅではなくウブントゥです
    却下!
    6
    ハモりながら ケンカしない
    アジいてむ んだぞ!
    一瞬くらい 検討して くださいよー!
    よけんな このっ
    ```

Although Google Lens scans from left to right, it still understands Japanese to be top-to-bottom-right-to-left, and you have to agree, it's THE MOST ACCURATE!  Kudos to [Google Cloud Vision](https://cloud.google.com/vision/docs/ocr) (at least, I want to belive it's using THIS API);  Overall, if you can go through [OAuth2](https://developers.google.com/identity/protocols/oauth2) and use online OCR, I highly recommend relying on Google Cloud Vision!  As for me, I need this application to be usable offline, hence I am going with Microsoft.

In the future, I may give it an option (via command line arg) to choose between offline (Microsoft) and online (Google Cloud Vision via OAuth2) - if you search my other github (Rust) projects, I have an OAuth2 + Google Cloud Vision somewhere...

### Comparision

Just a quick comparison compilation:

![compare-table](assets/ocr-comparison-table.png)

### windows-rs

For windows.Media.Ocr integrations with rust, you'll need to at least enable 2 features `Media_Ocr` and `Globalization`.  You can click on the [Feature Search](https://microsoft.github.io/windows-rs/features/) link in the [crates.io](https://crates.io/crates/windows) page to search for what other features you'd need:

    ```bash
    $ cargo add windows-rs --features Media_Ocr,Globalization,Storage_Streams
    ```
