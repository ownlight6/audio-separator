#[cfg(not(embed_model))]
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(not(embed_model))]
const DEFAULT_MODEL_URL: &str =
    "https://github.com/TRvlvr/model_repo/releases/download/all_public_uvr_models/Kim_Vocal_1.onnx";
const DEFAULT_MODEL_FILENAME: &str = "Kim_Vocal_1.onnx";

#[cfg(embed_model)]
const EMBEDDED_MODEL: &[u8] = include_bytes!("../Kim_Vocal_1.onnx");

pub fn default_model_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("audio-separator")
        .join("models")
}

pub fn ensure_model(
    model_path: Option<&Path>,
    _progress: Option<Box<dyn Fn(u64, u64) + Send>>,
) -> Result<PathBuf, String> {
    // 1. Explicit path from user
    if let Some(p) = model_path {
        if p.exists() {
            return Ok(p.to_path_buf());
        }
        return Err(format!("Model file not found: {}", p.display()));
    }

    // 2. Check current directory
    let local_path = PathBuf::from(DEFAULT_MODEL_FILENAME);
    if local_path.exists() {
        return Ok(local_path);
    }

    // 3. Check cache directory
    let dir = default_model_dir();
    let cached_path = dir.join(DEFAULT_MODEL_FILENAME);
    if cached_path.exists() {
        return Ok(cached_path);
    }

    // 4. Use embedded model if compiled in, otherwise download
    #[cfg(embed_model)]
    {
        return extract_embedded_model(&cached_path);
    }

    #[cfg(not(embed_model))]
    {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create model directory: {}", e))?;
        download_model(&cached_path, _progress)?;
        Ok(cached_path)
    }
}

#[cfg(embed_model)]
fn extract_embedded_model(dest: &Path) -> Result<PathBuf, String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create model directory: {}", e))?;
    }
    std::fs::write(dest, EMBEDDED_MODEL)
        .map_err(|e| format!("Failed to extract embedded model: {}", e))?;
    Ok(dest.to_path_buf())
}

#[cfg(not(embed_model))]
fn download_model(
    dest: &Path,
    progress: Option<Box<dyn Fn(u64, u64) + Send>>,
) -> Result<(), String> {
    let response = ureq::get(DEFAULT_MODEL_URL)
        .call()
        .map_err(|e| format!("Failed to download model: {}", e))?;

    let total_size: u64 = response
        .header("Content-Length")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let tmp_path = dest.with_extension("onnx.tmp");
    let mut file = std::fs::File::create(&tmp_path)
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut reader = response.into_reader();
    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 65536];

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download read error: {}", e))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Write error: {}", e))?;
        downloaded += n as u64;
        if let Some(ref cb) = progress {
            cb(downloaded, total_size);
        }
    }

    file.flush()
        .map_err(|e| format!("Flush error: {}", e))?;
    drop(file);

    std::fs::rename(&tmp_path, dest)
        .map_err(|e| format!("Failed to rename temp file: {}", e))?;

    Ok(())
}
