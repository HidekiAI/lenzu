// Based off of windows.Media.Ocr crates
use anyhow::Error;

use windows::{
    core::*,
    Globalization::Language,
    Graphics::Imaging::BitmapDecoder,
    Media::Ocr::OcrEngine,
    Storage::{FileAccessMode, StorageFile},
};
use crate::ocr_traits::OcrTrait;

const JAPANESE_LANGUAGE: &str = "ja";
//const JAPANESE_LANGUAGE_ID = windows::Win32::System::SystemServices::LANG_JAPANESE;

pub struct OcrWinMedia {}

impl OcrTrait for OcrWinMedia {
    fn new() -> Self {
        OcrWinMedia {}
    }

    fn init(&self) -> Vec<String> {
        let mut langs = Vec::new();
        langs.push(JAPANESE_LANGUAGE.to_string());
        langs
    }

    fn evaluate_by_paths(&self, image_path: &str) -> core::result::Result<String, Error> {
        let img = image::open(image_path).unwrap();
        self.evaluate(&img)
    }

    fn evaluate(&self, image: &image::DynamicImage) -> core::result::Result<String, Error> {
        todo!("CODE ME!");
        Ok("CODE ME!".to_string())
    }
}

fn test_main() -> Result<()> {
    // let's make sure JP is supported in desktop/user profile:
    let hstr: HSTRING = HSTRING::from(JAPANESE_LANGUAGE);
    let japanese_language: Language =
        Language::CreateLanguage(&hstr).expect("Failed to create Language");
    let profile_valid = OcrEngine::IsLanguageSupported(&japanese_language)
        .expect("Japanese is not installed in your profile");
    if profile_valid == false {
        panic!("Japanese is not installed in your profile");
    }
    // arg1: filename (full paths)
    let png_paths = std::env::args().nth(1).unwrap();
    futures::executor::block_on(main_async(png_paths.clone().as_str(), &japanese_language))
}

// NOTE: Paths passed needs to match the path separator of the OS, hence
// if you pass in for example "media/foo.png" on Windows, it will fail!
async fn main_async(png_paths: &str, language: &Language) -> Result<()> {
    let mut arg_image_path = String::new();
    // for windows, replace all occurances of '/' with "\\"
    if cfg!(target_os = "windows") {
        println!("Windows: Evaluating '{:?}' for forward-slashes", png_paths);
        for c in png_paths.chars() {
            if c == '/' {
                arg_image_path.push_str("\\");
            } else {
                arg_image_path.push(c);
            }
        }
    } else {
        println!("Linux: Evaluating '{:?}' for back-slashes", png_paths);
        // for linux, replace all occurances of '\' with "/"
        for c in png_paths.chars() {
            if c == '\\' {
                arg_image_path.push('/');
            } else {
                arg_image_path.push(c);
            }
        }
    }
    let mut message = std::env::current_dir().unwrap();
    message.push(arg_image_path);
    let file =
        StorageFile::GetFileFromPathAsync(&HSTRING::from(message.to_str().unwrap()))?.await?;
    let stream = file.OpenAsync(FileAccessMode::Read)?.await?;

    let decode = BitmapDecoder::CreateAsync(&stream)?.await?;
    let bitmap = decode.GetSoftwareBitmapAsync()?.await?;

    //let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    let engine = OcrEngine::TryCreateFromLanguage(language)?;
    //let result = engine.RecognizeAsync(&bitmap)?.await?;
    if let Ok(result) = engine.RecognizeAsync(&bitmap)?.await {
        println!("{}", result.Text()?);
    } else {
        panic!("Failed to recognize text");
    }

    Ok(())
}
