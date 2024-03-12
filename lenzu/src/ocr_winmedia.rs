// Based off of windows.Media.Ocr crates
use anyhow::Error;
//use futures::sink::Buffer;
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};

use crate::ocr_traits::OcrTrait;
use windows::{
    core::*,
    Globalization::Language,
    Graphics::Imaging::BitmapDecoder,
    Media::Ocr::OcrEngine,
    Storage::{
        FileAccessMode, StorageFile,
        Streams::{DataWriter, IBuffer, IRandomAccessStream, InMemoryRandomAccessStream},
    },
    Win32::System::SystemServices::LANG_JAPANESE,
};

const JAPANESE_LANGUAGE: &str = "ja";
//const JAPANESE_LANGUAGE_ID = windows::Win32::System::SystemServices::LANG_JAPANESE;

pub struct OcrWinMedia {
    language: Language,
}

impl OcrTrait for OcrWinMedia {
    fn new() -> Self
    where
        Self: Sized,
    {
        OcrWinMedia {
            language: Language::CreateLanguage(&HSTRING::from(JAPANESE_LANGUAGE))
                .expect("Failed to create Language"),
        }
    }

    fn init(&self) -> Vec<String> {
        let mut langs = Vec::new();
        langs.push(JAPANESE_LANGUAGE.to_string());
        let profile_valid = OcrEngine::IsLanguageSupported(&self.language)
            .expect("Japanese is not installed in your profile");
        if profile_valid == false {
            panic!("Japanese is not installed in your profile");
        }
        langs
    }

    fn evaluate_by_paths(&self, image_path: &str) -> core::result::Result<String, Error> {
        let img = image::open(image_path).unwrap();
        self.evaluate(&img)
    }

    fn evaluate(&self, image: &image::DynamicImage) -> core::result::Result<String, Error> {
        let mut raw_buffer_u8: Vec<u8> = Vec::new();
        let cursor = std::io::Cursor::new(&mut raw_buffer_u8);

        // Write the image to the Cursor<Vec<u8>>, which is a 'memory stream'
        let encoder = PngEncoder::new(cursor);
        let width = image.width();
        let height = image.height();
        let color_type = ColorType::from(image.color());
        encoder.write_image(
            image.clone().into_bytes().as_slice(),  // have to clone (unfortunately)
            width,
            height,
            color_type,
        )?;

        let in_memory_stream =
            futures::executor::block_on(self.slice_to_memstream(&raw_buffer_u8))?;
        match futures::executor::block_on(self.evaluate_async(&self.language, in_memory_stream)) {
            Ok(s) => Ok(s),
            Err(e) => Err(e.into()),
        }
    }
}

impl OcrWinMedia {
    fn test_main(&self) -> Result<String> {
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
        let ret = futures::executor::block_on(
            self.evaluate_async_path(png_paths.clone().as_str(), &japanese_language),
        );
        ret
    }

    // NOTE: Paths passed needs to match the path separator of the OS, hence
    // if you pass in for example "media/foo.png" on Windows, it will fail!
    async fn evaluate_async_path(&self, png_paths: &str, language: &Language) -> Result<String> {
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
        let file_stream = file.OpenAsync(FileAccessMode::Read)?.await?;
        let in_memory_stream = self.fstream_to_memstream(file_stream)?;
        futures::executor::block_on(self.evaluate_async(language, in_memory_stream))
    }

    fn fstream_to_memstream(
        &self,
        stream: IRandomAccessStream,
    ) -> Result<InMemoryRandomAccessStream> {
        let wss_buffer = windows::Storage::Streams::Buffer::Create(stream.Size()? as u32)?;
        let in_memory_stream = InMemoryRandomAccessStream::new()?;
        in_memory_stream.WriteAsync(&wss_buffer)?;
        Ok(in_memory_stream)
    }

    async fn slice_to_memstream(&self, slice: &[u8]) -> Result<InMemoryRandomAccessStream> {
        let in_memory_stream: InMemoryRandomAccessStream = InMemoryRandomAccessStream::new()?;
        let data_writer: DataWriter = DataWriter::CreateDataWriter(&in_memory_stream)?;
        data_writer.WriteBytes(slice)?;
        match data_writer.StoreAsync()?.await {
            Ok(bytes_written) => {
                println!(
                    "slice_to_memstream() - Bytes written: {} (slice size: {} bytes)",
                    bytes_written,
                    slice.len()
                );
                let _ = in_memory_stream.Seek(0);
                Ok(in_memory_stream)
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn evaluate_async(
        &self,
        language: &Language,
        stream: InMemoryRandomAccessStream,
    ) -> Result<String> {
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

        // somehow it did not panic and succeeded, at least if that is the case, return empty string
        Ok("".to_string())
    }
}
