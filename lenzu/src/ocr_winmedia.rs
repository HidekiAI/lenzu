use std::fmt::format;

// Based off of windows.Media.Ocr crates
use anyhow::Error;
use futures::{FutureExt, TryFutureExt};
//use futures::sink::Buffer;
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
use winapi::shared::cfg;

use crate::ocr_traits::{self, OcrTrait, OcrTraitResult};
use tokio::time::{timeout, Duration};
use windows::{
    core::*,
    Globalization::Language,
    Graphics::Imaging::BitmapDecoder,
    Media::Ocr::{OcrEngine, OcrResult},
    Storage::{
        FileAccessMode, StorageFile,
        Streams::{DataReader, DataWriter, IRandomAccessStream, InMemoryRandomAccessStream},
    },
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

    fn evaluate_by_paths(
        &self,
        image_path: &str,
    ) -> core::result::Result<ocr_traits::OcrTraitResult, Error> {
        let img = image::open(image_path).unwrap();
        self.evaluate(&img)
    }

    fn evaluate(
        &self,
        image: &image::DynamicImage,
    ) -> core::result::Result<ocr_traits::OcrTraitResult, Error> {
        let mut raw_buffer_u8: Vec<u8> = Vec::new();
        let cursor = std::io::Cursor::new(&mut raw_buffer_u8);

        // Write the image to the Cursor<Vec<u8>>, which is a 'memory stream'
        let encoder = PngEncoder::new(cursor);
        let width = image.width();
        let height = image.height();
        let color_type = ColorType::from(image.color());
        match encoder.write_image(
            image.clone().into_bytes().as_slice(), // have to clone (unfortunately)
            width,
            height,
            color_type,
        ) {
            Ok(_) => {
                let in_memory_stream_transform_result =
                    futures::executor::block_on(self.slice_to_memstream(&raw_buffer_u8));
                match in_memory_stream_transform_result {
                    Ok(in_memory_stream) => {
                        match futures::executor::block_on(
                            self.evaluate_async(&self.language, &in_memory_stream),
                        ) {
                            Ok(s) => Ok(s),
                            Err(e) => {
                                println!("#================= Error: {:?}\n\n", e.to_string());
                                Err(e.into())
                            }
                        }
                    }
                    Err(e) => Err(e.into()),
                }
            }
            Err(e) => {
                println!("Error: {:?}", e);
                return Err(e.into());
            }
        }
    }
}

impl OcrWinMedia {
    pub fn test_seek_multiple(&self) -> Result<OcrTraitResult> {
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
        let png_paths = match std::env::args().len() {
            2 => std::env::args().nth(1).unwrap(),
            _ => {
                // use default file...
                println!("Usage: {} <image_path>", std::env::args().nth(0).unwrap());
                // if assets directory exists on current dir, use that, else go one dir up
                if std::path::Path::new("assets").exists() {
                    "assets/ubunchu01_02.png".to_string()
                } else {
                    "../assets/ubunchu01_02.png".to_string()
                }
            }
        };
        let ret = futures::executor::block_on(
            self.evaluate_async_path(png_paths.clone().as_str(), &japanese_language),
        );
        // now seek back to 0, and transform to memory stream
        let file_stream =
            futures::executor::block_on(self.get_filestream(png_paths.as_str())).unwrap();
        // NOTE: in_memory_stream here will not be async since it's not awaited
        let in_memory_stream =
            futures::executor::block_on(self.fstream_to_memstream(&file_stream)).unwrap();

        // as a test, write it once with no seek(0), then write it  again, then seek(0) and write again twice
        println!(
            "\n###############\nDumping to test_seek_multiple_1.png - {:?}",
            in_memory_stream.Position().unwrap()
        );
        futures::executor::block_on(Self::dump_stream_to_png(
            &in_memory_stream,
            "test_seek_multiple_1.png",
        ));
        println!(
            "\n###############\nDumping to test_seek_multiple_2.png - {:?}",
            in_memory_stream.Position().unwrap()
        );
        futures::executor::block_on(Self::dump_stream_to_png(
            &in_memory_stream,
            "test_seek_multiple_2.png",
        ));
        let _ = in_memory_stream.Seek(0);
        println!(
            "\n###############\nDumping to test_seek_multiple_3.png - {:?}",
            in_memory_stream.Position().unwrap()
        );
        futures::executor::block_on(Self::dump_stream_to_png(
            &in_memory_stream,
            "test_seek_multiple_3.png",
        ));
        println!(
            "\n###############\nDumping to test_seek_multiple_4.png - {:?}",
            in_memory_stream.Position().unwrap()
        );
        futures::executor::block_on(Self::dump_stream_to_png(
            &in_memory_stream,
            "test_seek_multiple_4.png",
        ));
        ret
    }

    // NOTE: Paths passed needs to match the path separator of the OS, hence
    // if you pass in for example "media/foo.png" on Windows, it will fail!
    async fn evaluate_async_path(
        &self,
        png_paths: &str,
        language: &Language,
    ) -> Result<OcrTraitResult> {
        let file_stream_result = self.get_filestream(png_paths).await;
        match file_stream_result {
            Ok(file_stream) => {
                let in_memory_stream_result = self.fstream_to_memstream(&file_stream);
                match in_memory_stream_result.await {
                    Ok(in_memory_stream) => {
                        let eval_result = self.evaluate_async(language, &in_memory_stream).await;
                        eval_result
                    }
                    Err(e) => Err(e.into()),
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn get_filestream(&self, png_paths: &str) -> Result<IRandomAccessStream> {
        let mut arg_image_path = String::new();
        // for windows, replace all occurances of '/' with "\\"
        // see 'https://learn.microsoft.com/en-us/uwp/api/windows.storage.storagefile.getfilefrompathasync' for more details
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
        let mut absolute_filepaths_buf: std::path::PathBuf = std::env::current_dir().unwrap();
        absolute_filepaths_buf.push(arg_image_path);
        absolute_filepaths_buf = absolute_filepaths_buf.canonicalize().unwrap(); // make sure it's absolute path without ".." or "."
                                                                                 // on Windows, canonicalize() will convert  who "C:\foo\..\bar" to  "\\?C:\bar" (UNC path?), so remove the "\\?\" prefix
        if cfg!(target_os = "windows") {
            absolute_filepaths_buf = std::path::PathBuf::from(
                absolute_filepaths_buf
                    .to_str()
                    .unwrap()
                    .replace("\\\\?\\", ""),
            );
        }

        //let storage_file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path_buf.to_str().unwrap()))?.await;
        let storage_file_result = StorageFile::GetFileFromPathAsync(&HSTRING::from(
            absolute_filepaths_buf.to_str().unwrap(),
        ));
        match storage_file_result {
            Ok(storage_file_operation) => {
                // verify file exists
                println!("get_filestream(): Verifying if absolute file paths '{}' exists and/or valid...",
                    absolute_filepaths_buf.to_str().unwrap()
                );
                let storage_file_op_result = storage_file_operation.await;
                match storage_file_op_result {
                    Ok(storage_file) => {
                        // let file_stream_result = storage_file.OpenAsync(FileAccessMode::Read)?.await;
                        let open_result = storage_file.OpenAsync(FileAccessMode::Read);
                        match open_result {
                            Ok(open_operation) => {
                                println!("get_filestream(): Opening file...");
                                let open_file_stream_result = open_operation.await;
                                match open_file_stream_result {
                                    Ok(file_stream) => {
                                        println!("get_filestream(): File opened...");
                                        Ok(file_stream)
                                    }
                                    Err(e) => {
                                        println!("Error (get_filestream(match: open_file_stream_result)): Failed to OpenAsync() - {:?}", e);
                                        Err(e.into())
                                    }
                                }
                            }
                            Err(e) => {
                                println!("Error (get_filestream(match: open_result)): {:?}", e);
                                return Err(e.into());
                            }
                        }
                    }
                    Err(e) => {
                        println!(
                            "Error (get_filestream(match: storage_file_op_result)): {:?}",
                            e
                        );
                        Err(e.into())
                    }
                }
            }
            Err(e) => {
                println!(
                    "Error (get_filestream(match: storage_file)): {:?} - paths: '{}",
                    e,
                    absolute_filepaths_buf.to_str().unwrap()
                );
                Err(e.into())
            }
        }
    }

    async fn fstream_to_memstream(
        &self,
        fstream: &IRandomAccessStream,
    ) -> Result<InMemoryRandomAccessStream> {
        // first, check if stream has been closed, and if so, panic
        if fstream.CanRead().unwrap() == false {
            panic!("Error: stream is closed");
        }

        fstream.Seek(0)?; // reset seek position to 0 so we can read the ENTIRE stream
        let stream_size = fstream.Size()?;
        if stream_size == 0 {
            panic!("Error: stream_size is 0");
        }

        println!(
            "fstream_to_memstream(): Stream size: {} bytes to be allocated to memory buffer...",
            stream_size
        );
        fstream.Seek(0)?; // reset seek position to 0 (Size() will move the seek position to the end of the stream, so reset it back to 0)

        // create DataReader to read from file stream
        let reader = DataReader::CreateDataReader(fstream)
            .expect(format!("Failed to create DataReader for fstream").as_str());
        let bytes_read = reader
            .LoadAsync(stream_size as u32)
            .expect(format!("Failed to load {} bytes from fstream", stream_size).as_str())
            .await
            .expect(format!("Failed to load {} bytes from fstream", stream_size).as_str());
        println!(
            "fstream_to_memstream(): Buffer loaded {} bytes (out of {} bytes)...",
            bytes_read, stream_size
        );
        let buffer = reader
            .ReadBuffer(stream_size as u32)
            .expect(format!("Failed to read {} bytes from fstream", stream_size).as_str());

        let local_buffer_reader = DataReader::FromBuffer(&buffer)
            .expect(format!("Failed to create DataReader from buffer").as_str());
        let buffer_length = local_buffer_reader
            .UnconsumedBufferLength()
            .expect(format!("Failed to get UnconsumedBufferLength").as_str());

        let mut data = vec![0u8; buffer_length as usize];
        local_buffer_reader
            .ReadBytes(&mut data)
            .expect(format!("Failed to read {} bytes from buffer", buffer_length).as_str());

        // hopefully, we'll be able to transfer ownership of his in_memory_stream to the caller
        let in_memory_stream = self
            .slice_to_memstream(&data)
            .await
            .expect(format!("Failed to transform data to InMemoryRandomAccessStream from {} bytes of vec/array data", buffer_length).as_str());

        println!("fstream_to_memstream(): Memory Stream creation succeeded...");
        let initial_position = in_memory_stream.Position();
        let initial_seek_result = in_memory_stream.Seek(0);
        let stream_size = in_memory_stream.Size();
        let post_size_postion = in_memory_stream.Position();
        let seek_result = in_memory_stream.Seek(0);
        let post_seek_position = in_memory_stream.Position();
        println!(
            "fstream_to_memstream(): Initial position: {:?}, initial Seek result: {:?}",
            initial_position,
            initial_seek_result.is_ok(),
        );

        println!(
            "fstream_to_memstream(): Stream size: {:?} bytes, position: {:?}",
            stream_size, post_size_postion,
        );
        println!(
            "fstream_to_memstream(): Seek result: Ok{:?}, Stream seek position: {:?}",
            seek_result.is_ok(),
            post_seek_position,
        );
        if initial_position.is_err() {
            panic!("Error: '{:?}'", initial_position)
        }
        println!("fstream_to_memstream(): >> Stream created...");

        return Ok(in_memory_stream);
    }

    // I do not know any other way to do this, but this method is quite inefficient, as it reads the entire stream into a buffer
    // and then returns the buffer.  This is not a problem if the stream is small, but if the stream is large, then this method
    // will consume a lot of memory.  I have tried many different methods, but this is the only one that works.
    // when it all comes down to it, it's because the stream is an async stream, and the only way to read from it is to
    // use LoadAsync() and ReadBuffer() combination
    async fn copy_stream_to_vec(in_memory_stream: &InMemoryRandomAccessStream) -> Vec<u8> {
        // first, let's make sure we reset the stream back to the head of the buffer
        in_memory_stream
            .Seek(0)
            .expect(format!("Failed to seek to 0 in in_memory_stream").as_str());

        // Get the size of the stream (there were some documentations (no reference)) that says Size() will NOT set the position to tail of the stream
        let stream_size = in_memory_stream
            .Size()
            .expect(format!("Failed to get Size() of in_memory_stream").as_str());
        println!(
            "copy_stream_to_vec(): Stream size: {} bytes (allocating Vec<u8> of this size)",
            stream_size
        );
        in_memory_stream
            .Seek(0)
            .expect(format!("Failed to seek to 0 in in_memory_stream").as_str());
        // Create a DataReader attached to the stream now that stream have been reset to head
        let async_in_memory_stream_reader = DataReader::CreateDataReader(in_memory_stream)
            .expect(format!("Failed to create DataReader for attaching in_memory_stream").as_str());

        //let data_reader = DataReader::CreateDataReader(&input_stream_for_datareader).expect(
        //    format!("Failed to create DataReader for input_stream_for_datareader").as_str(),
        //);

        // Read the entire stream into a buffer
        let mut ret_buffer_vec = vec![0u8; (stream_size * 1) as usize];
        assert!(
            ret_buffer_vec.len() >= stream_size as usize,
            "Error: buffer is too small"
        );
        println!(
            "\ncopy_stream_to_vec(): Reading {} bytes from in_memory_stream via DataReader...",
            stream_size
        );
        // tried many different methods, in the end, the only way to read the stream is to use LoadAsync() and ReadBuffer() combination
        //data_reader
        //    .ReadBytes(&mut ret_buffer)
        //    .expect(format!("Failed to read {} bytes from in_memory_stream", stream_size).as_str());
        //reader
        //    .ReadBytes(&mut ret_buffer) // this will panic with 'Failed to read 302738 bytes from in_memory_stream: Error { code: HRESULT(0x8000000B), message: "The operation attempted to access data outside the valid range" }'
        //    .expect(format!("Failed to read {} bytes from in_memory_stream", stream_size).as_str());
        //reader
        //    .DetachStream()
        //    .expect(format!("Failed to detach stream from reader").as_str());
        //for index in 0..stream_size {
        //    println!("{}", index);
        //    let byte = data_reader
        //        .ReadByte()
        //        .expect("Failed to read byte from data_reader");
        //    ret_buffer.push(byte as u8);
        //}
        let async_bytes_cached = async_in_memory_stream_reader
            .LoadAsync(stream_size as u32)
            .expect(
                format!(
                    "Failed to prepare for loading {} bytes from in_memory_stream",
                    stream_size
                )
                .as_str(),
            )
            .await
            .expect(format!("Failed to load {} bytes from in_memory_stream", stream_size).as_str());
        assert!(
            async_bytes_cached == stream_size as u32,
            "Error: bytes_read != stream_size"
        );
        // now move the cached-data to buffer before some other async operation  takes effect...
        let local_buffer_from_cache = async_in_memory_stream_reader
            .ReadBuffer(stream_size as u32)
            .expect(format!("Failed to read {} bytes from in_memory_stream", stream_size).as_str());

        // now that the stream has been read, we can detach the stream from the reader
        // IT IS IMPORTANT TO DETACH THE STREAM FROM THE READER, OTHERWISE, THE STREAM WILL BE CLOSED!!!!!!
        // AND IT HAS TO BE DETACHED AFTER CALLING ReadBuffer()
        async_in_memory_stream_reader
            .DetachStream()
            .expect(format!("Failed to detach stream from reader").as_str());

        // and then, transfer the data from the local_buffer_from_cache to ret_buffer_vec
        let from_buffer_reader = DataReader::FromBuffer(&local_buffer_from_cache)
            .expect(format!("Failed to create DataReader from buffer").as_str());
        let unconsumed_buffer_length = from_buffer_reader
            .UnconsumedBufferLength()
            .expect(format!("Unable to resolve UnconsumedBufferLength()").as_str())
            as usize;
        assert!(
            unconsumed_buffer_length == stream_size as usize,
            "Error: length != stream_size"
        );
        from_buffer_reader
            .ReadBytes(&mut ret_buffer_vec)
            .expect(format!("Failed to read {} bytes from in_memory_stream", stream_size).as_str());
        println!(
            "\ncopy_stream_to_vec(): Reading done...  Bytes read: {} bytes\n\n",
            async_bytes_cached
        );

        // before we leave, reset again the stream back to the head of the buffer
        // NOTE: The calling methods currently, when they call Seek(0), it will panic, but it will not panic if it's called here
        in_memory_stream
            .Seek(0)
            .expect(format!("Failed to seek to 0 in in_memory_stream").as_str());

        ret_buffer_vec
    }

    async fn dump_stream_to_png(in_memory_stream: &InMemoryRandomAccessStream, filename: &str) {
        println!("\nDumping to {}", filename);
        // first, check if stream has been closed, and if so, panic
        if in_memory_stream.CanRead().unwrap() == false {
            panic!("Error: stream is closed");
        }
        let pos_result = in_memory_stream.Position();
        let pos = match pos_result {
            Ok(p) => {
                if cfg!(debug_assertions) {
                    println!("DEBUG: dump_stream_to_png(): Stream position: {:?}", p);
                }
                p
            }

            Err(e) => {
                // stream is probably closed...
                panic!("Error: {:?}", e)
            }
        };
        if cfg!(debug_assertions) {
            println!("DEBUG: dump_stream_to_png(): Stream position: {:?}", pos);
            in_memory_stream.Seek(pos).unwrap(); // interesting to find out if pos is not 0, what the Size would be...
            let debug_stream_size = in_memory_stream.Size().unwrap(); // need to re-seek position once size is read
            println!(
                "DEBUG: dump_stream_to_png(): Stream size: {:?} bytes, position: {:?}",
                debug_stream_size, pos
            );
        }
        in_memory_stream.Seek(0).unwrap(); // I think I have to force seek to 0, because Position() is equal to Size() (which is the end of the stream)
        let stream_size = in_memory_stream.Size().unwrap(); // need to re-seek position once size is read

        println!("dump_stream_to_png(): Stream size: {:?} bytes, position: {:?} - begin creating DataReader..." , stream_size, pos);
        let data_from_stream = Self::copy_stream_to_vec(in_memory_stream).await;
        // Now `data` is a Vec<u8> that you can use as a byte slice
        let data_slice = &data_from_stream[..];
        // make sure slice is more than 1 byte
        if data_slice.len() == 0 {
            panic!("Error: data_slice is empty");
        }
        // interestingly, the last thing copy_stream_to_vec() does is to reset the stream position to 0, yet if I tried to reset here, it will panic
        in_memory_stream.Seek(0).unwrap(); // if it doesn't panic here, we've got something working...

        let img = image::load_from_memory(&data_slice).unwrap();
        img.save(filename).unwrap();
        println!(">> Dumped done...");

        if in_memory_stream.Position().unwrap() != 0 {
            println!(">>> Resetting stream position to 0...");
            let _ = in_memory_stream.Seek(0);
        }

        // just in case, make sure stream is still valid/open, maybe DataReader may have implicitly closed it?
        if cfg!(debug_assertions) {
            println!(
                "DEBUG: dump_stream_to_png(): Stream position: {:?}\n\n",
                in_memory_stream.Position().unwrap(), // it should panic if stream is closed...
            );
        }
    }

    async fn slice_to_memstream(&self, slice: &[u8]) -> Result<InMemoryRandomAccessStream> {
        let in_memory_stream_result = InMemoryRandomAccessStream::new();
        let in_memory_stream = match in_memory_stream_result {
            Ok(stream) => stream,
            Err(e) => {
                println!("Error: {:?}", e);
                return Err(e.into());
            }
        };
        // NOTE: Problem (it seems) of having logic within the match block is that when it goes out of scope,
        // stream (in_memory_stream) will get closed?
        let data_writer = DataWriter::CreateDataWriter(&in_memory_stream)
            .expect(format!("Failed to create DataWriter for in_memory_stream").as_str());
        println!("slice_to_memstram(): DataWriter created...");
        let _ = data_writer
            .WriteBytes(slice)
            .expect(format!("Failed to write slice to DataWriter").as_str());

        println!("slice_to_memstram(): Data written...");
        let _ = match data_writer.FlushAsync() {
            Ok(flush_operation) => match flush_operation.await {
                Ok(flushed) => {
                    println!("slice_to_memstram(): Flushed: {:?}", flushed);
                }
                Err(e) => println!("FlushOp failed: {:?}", e),
            },
            Err(e) => println!("FlushAsync failed: {:?}", e),
        };

        let bytes_written = data_writer
            .StoreAsync()
            .expect(format!("Failed to call StoreAsync").as_str())
            .await
            .expect(format!("StoreAsync (upon unblocked) failed to write data").as_str());

        let _ = match data_writer.FlushAsync() {
            Ok(flush_operation) => match flush_operation.await {
                Ok(flushed) => {
                    println!("slice_to_memstram(): Flushed: {:?}", flushed);
                }
                Err(e) => {
                    println!("FlushOp failed: {:?}", e)
                }
            },
            Err(e) => {
                println!("FlushAsync failed: {:?}", e)
            }
        };

        println!(
            "slice_to_memstream() - Bytes written: {} (slice size: {} bytes)",
            bytes_written,
            slice.len()
        );

        // NOTE: DETACH STREAM FROM WRITER, OTHERWISE, THE STREAM WILL BE CLOSED!!!!!!
        // all is done, now detach the stream from the writer
        data_writer
            .DetachStream()
            .expect(format!("Failed to detach stream from writer").as_str());

        // ######################## DEBUG BEGIN: dump some info  if in DEBUG build:
        if cfg!(debug_assertions) {
            if in_memory_stream.CanRead().unwrap() == false {
                panic!("Error: stream is closed");
            }
            println!(
                "DEBUG: slice_to_memstream() - Stream size: {:?} bytes, position: {:?}",
                in_memory_stream.Size(),
                in_memory_stream.Position()
            );
            if in_memory_stream.CanRead().unwrap() == false {
                panic!("Error: stream is closed");
            }
            // for debug, dump this as a PNG file
            in_memory_stream.Seek(0).unwrap();
            Self::dump_stream_to_png(&in_memory_stream, "debug_slice_to_memstream_001.png").await;
            if in_memory_stream.CanRead().unwrap() == false {
                panic!("Error: stream is closed");
            }
            in_memory_stream.Seek(0).unwrap(); // why does it panic here?
            if in_memory_stream.CanRead().unwrap() == false {
                panic!("Error: stream is closed");
            }
            // we dump twice to verify that no matter how many times we dump, the READ stream is still valid
            Self::dump_stream_to_png(&in_memory_stream, "debug_slice_to_memstream_002.png").await;
            if in_memory_stream.CanRead().unwrap() == false {
                panic!("Error: stream is closed");
            }
            in_memory_stream.Seek(0).unwrap();
        }
        // ######################## DEBUG END

        // reset seek position to 0 because Position() is currently equali to Size()
        if in_memory_stream.CanRead().unwrap() == false {
            panic!("Error: stream is closed");
        }
        let seek_result = in_memory_stream.Seek(0);
        match seek_result {
            Ok(_) => {
                // dump some info  if in DEBUG build:
                if cfg!(debug_assertions) {
                    println!( "DEBUG: slice_to_memstream() - Seek result: Ok({:?}), Stream seek position: {:?} (Size: {:?})",
                        seek_result.is_ok(),
                        in_memory_stream.Position(),
                        in_memory_stream.Size(),
                    );
                }
                println!(
                    "slice_to_memstram(): >> Memory Stream created... Position={:?}",
                    in_memory_stream.Position()
                );
                // transfer ownership of in_memory_stream to the caller
                if in_memory_stream.CanRead().unwrap() == false {
                    panic!("Error: stream is closed");
                }
                Ok(in_memory_stream)
            }
            Err(e) => {
                panic!("Error: {:?}", e);
            }
        }
    }

    async fn create_decoder_with_timeout(
        stream: &InMemoryRandomAccessStream,
        timeout_in_seconds: u64,
    ) -> Result<BitmapDecoder> {
        // first, check if stream has been closed, and if so, panic
        if stream.CanRead().unwrap() == false {
            panic!("Error: stream is closed");
        }

        match stream.Position() {
            Ok(pos) => {
                println!("create_decoder_with_timeout(): Stream position: {}", pos);
                if pos != 0 {
                    println!("create_decoder_with_timeout(): Resetting stream position to 0...");
                    let _ = stream.Seek(0);
                }
            }
            Err(e) => {
                println!("Error: {:?}", e);
            }
        }
        let create_decoder_future_result = BitmapDecoder::CreateAsync(stream);

        // Error: 'there is no reactor running, must be called from the context of a Tokio 1.x runtime'
        match create_decoder_future_result {
            Ok(create_decoder_future) => {
                match timeout(
                    Duration::from_secs(timeout_in_seconds),
                    create_decoder_future,
                )
                .await
                {
                    Ok(result) => result,
                    Err(err_elapsed) => panic!("Timeout: {:?}", err_elapsed),
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn evaluate_async(
        &self,
        language: &Language,
        stream: &InMemoryRandomAccessStream,
    ) -> Result<ocr_traits::OcrTraitResult> {
        // first, check if stream has been closed, and if so, panic
        if stream.CanRead().unwrap() == false {
            panic!("Error: stream is closed");
        }

        println!(
            "Evaluating (OCR recognizing) for lanaguage: {:?}",
            language.LanguageTag()?
        );
        let timeout_in_seconds = 15;
        let duration: Duration = Duration::from_secs(timeout_in_seconds);
        println!("evaluate_async(): Creating BitmapDecoder...");
        let decode: BitmapDecoder =
            match Self::create_decoder_with_timeout(stream, timeout_in_seconds).await {
                Ok(decoder) => decoder,
                Err(e) => {
                    println!("Error: {:?}", e);
                    return Err(e.into());
                }
            };
        println!("evaluate_async(): Stream created...");
        //let bitmap: windows::Graphics::Imaging::SoftwareBitmap =
        //    match decode.GetSoftwareBitmapAsync()?.await {
        //        Ok(bitmap) => bitmap,
        //        Err(e) => {
        //            println!("Error: {:?}", e);
        //            return Err(e.into());
        //        }
        //    };
        let bitmap: windows::Graphics::Imaging::SoftwareBitmap =
            timeout(duration.clone(), decode.GetSoftwareBitmapAsync().unwrap())
                .await
                .unwrap()
                .unwrap();
        println!("evaluate_async(): Bitmap created...");

        let engine: OcrEngine = match OcrEngine::TryCreateFromUserProfileLanguages() {
            Ok(engine) => engine,
            Err(e) => {
                println!("Error: {:?}", e);
                return Err(e.into());
            }
        };
        println!("evaluate_async(): Engine created...  time started...");
        let start = std::time::Instant::now();
        //let ocr_result: std::prelude::v1::Result<OcrResult, windows::core::Error> = engine.RecognizeAsync(&bitmap)?.await;
        let ocr_result = timeout(duration.clone(), engine.RecognizeAsync(&bitmap).unwrap())
            .await
            .unwrap();
        println!(
            "evaluate_async(): OCR took: {} mSec, result (OK?): {}",
            start.elapsed().as_millis(),
            ocr_result.is_ok()
        );

        if let Ok(result) = ocr_result {
            let str_block: String = result.Text().unwrap().to_string();
            let lines: Vec<String> = result
                .Lines()
                .unwrap()
                .into_iter()
                .map(|x| x.Text().unwrap().to_string())
                .collect::<Vec<_>>();

            println!("evaluate_async():\n{}", str_block);
            let trait_result = OcrTraitResult {
                text: str_block,
                lines: lines,
            };
            println!(
                "evaluate_async(): Recognized text: {:?}",
                trait_result.lines
            );
            return Ok(trait_result);
        } else {
            //panic!("Failed to recognize text");
            return Err(ocr_result.unwrap_err().into());
        }
    }
}
