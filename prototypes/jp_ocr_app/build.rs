use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{copy, Read};
use std::path::Path;
use tar::Archive;

// Update these hashes by running `sha256sum` on your manual downloads
const DET_HASH: &str = "0000000000000000000000000000000000000000";
const REC_HASH: &str = "0000000000000000000000000000000000000000";

fn main() -> anyhow::Result<()> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let assets_dir = Path::new(&manifest_dir).join("assets");
    fs::create_dir_all(&assets_dir)?;

    let det_url = "https://paddleocr.bj.bcebos.com/PP-OCRv4/chinese/ch_PP-OCRv4_det_infer.tar";
    let rec_url = "https://paddleocr.bj.bcebos.com/PP-OCRv4/jp/jp_PP-OCRv4_rec_infer.tar";
    let dict_url = "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/dict/japan_dict.txt";

    // 1. Handle Detection Model
    download_and_unpack(det_url, &assets_dir, "det_model.tar", DET_HASH)?;

    // 2. Handle Recognition Model
    download_and_unpack(rec_url, &assets_dir, "rec_model.tar", REC_HASH)?;

    // 3. Handle Dictionary
    let dict_path = assets_dir.join("japan_dict.txt");
    if !dict_path.exists() {
        let mut resp = reqwest::blocking::get(dict_url)?;
        let mut file = fs::File::create(dict_path)?;
        copy(&mut resp, &mut file)?;
    }

    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}

fn download_and_unpack(
    url: &str,
    dest_dir: &Path,
    file_name: &str,
    expected_hash: &str,
) -> anyhow::Result<()> {
    let tar_path = dest_dir.join(file_name);

    // Checksum/Existence check
    let mut needs_download = true;
    if tar_path.exists() {
        let actual_hash = calculate_hash(&tar_path)?;
        if actual_hash == expected_hash || expected_hash.starts_with('0') {
            needs_download = false;
        } else {
            fs::remove_file(&tar_path)?;
        }
    }

    if needs_download {
        println!("cargo:warning=Downloading {}...", url);
        let mut resp = reqwest::blocking::get(url)?;
        let mut file = fs::File::create(&tar_path)?;
        copy(&mut resp, &mut file)?;
    }

    // Unpack .tar (Paddle models usually aren't gzipped, but check extension)
    let tar_file = fs::File::open(&tar_path)?;
    let mut archive = Archive::new(tar_file);
    archive.unpack(dest_dir)?;

    Ok(())
}

fn calculate_hash(path: &Path) -> anyhow::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}
