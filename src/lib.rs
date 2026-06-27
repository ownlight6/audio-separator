pub mod audio_io;
pub mod model_manager;
pub mod separator;

use std::path::{Path, PathBuf};
use std::sync::mpsc;

#[derive(Clone)]
pub struct SeparationResult {
    pub input_path: PathBuf,
    pub vocals_path: PathBuf,
    pub accompaniment_path: PathBuf,
    pub sample_rate: u32,
    pub duration_secs: f64,
}

#[derive(Clone, Debug)]
pub struct ProgressUpdate {
    pub stage: String,
    pub progress: f32,
}

pub fn separate_file(
    input: &Path,
    output_dir: &Path,
    model_path: Option<&Path>,
    progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
) -> Result<SeparationResult, String> {
    let send_progress = |stage: &str, progress: f32| {
        if let Some(ref tx) = progress_tx {
            let _ = tx.send(ProgressUpdate {
                stage: stage.to_string(),
                progress,
            });
        }
    };

    // 1. Ensure model is available
    send_progress("downloading_model", 0.0);
    let resolved_model = model_manager::ensure_model(
        model_path,
        {
            let tx = progress_tx.clone();
            Some(Box::new(move |downloaded, total| {
                if let Some(ref tx) = tx {
                    let p = if total > 0 {
                        downloaded as f32 / total as f32
                    } else {
                        0.0
                    };
                    let _ = tx.send(ProgressUpdate {
                        stage: "downloading_model".to_string(),
                        progress: p,
                    });
                }
            }))
        },
    )?;

    // 2. Decode audio
    send_progress("decoding", 0.0);
    let decoded = audio_io::decode_audio(input)?;
    send_progress("decoding", 1.0);

    let original_rate = decoded.sample_rate;

    // 3. Resample to 44100 if needed
    let (samples, work_rate) = if decoded.sample_rate != 44100 {
        send_progress("resampling", 0.0);
        let resampled = audio_io::resample(&decoded.samples, decoded.sample_rate, 44100)?;
        send_progress("resampling", 1.0);
        (resampled, 44100u32)
    } else {
        (decoded.samples, decoded.sample_rate)
    };

    let total_samples = samples[0].len();
    let duration_secs = total_samples as f64 / work_rate as f64;

    // 4. Run separation
    send_progress("separating", 0.0);
    let mut sep = separator::MdxSeparator::new(&resolved_model)?;
    let (vocals, accompaniment) = sep.separate(&samples, Some(&|p| {
        send_progress("separating", p);
    }))?;

    // 5. Resample back if needed
    let (vocals_out, accomp_out) = if original_rate != 44100 {
        send_progress("resampling_output", 0.0);
        let v = audio_io::resample(&vocals, 44100, original_rate)?;
        let a = audio_io::resample(&accompaniment, 44100, original_rate)?;
        send_progress("resampling_output", 1.0);
        (v, a)
    } else {
        (vocals, accompaniment)
    };

    // 6. Write output files
    send_progress("writing", 0.0);
    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    let stem = input
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let vocals_path = output_dir.join(format!("{}_vocals.wav", stem));
    let accomp_path = output_dir.join(format!("{}_accompaniment.wav", stem));

    audio_io::write_wav(&vocals_path, &vocals_out, original_rate)?;
    send_progress("writing", 0.5);
    audio_io::write_wav(&accomp_path, &accomp_out, original_rate)?;
    send_progress("writing", 1.0);

    Ok(SeparationResult {
        input_path: input.to_path_buf(),
        vocals_path,
        accompaniment_path: accomp_path,
        sample_rate: original_rate,
        duration_secs,
    })
}
