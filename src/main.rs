#![cfg_attr(all(feature = "gui", target_os = "windows"), windows_subsystem = "windows")]

#[cfg(feature = "gui")]
mod gui;

use clap::Parser;
use std::path::PathBuf;
use std::sync::mpsc;

#[cfg(all(target_os = "windows", embed_ort_dll))]
const EMBEDDED_ORT_DLL: &[u8] = include_bytes!("../onnxruntime.dll");

#[derive(Parser, Debug)]
#[command(
    name = "audio-separator",
    version,
    about = "Separate vocals from background music using AI"
)]
struct Args {
    /// Input audio file (MP3, FLAC, OGG)
    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,

    /// Output directory (default: same as input file)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Path to ONNX model file (downloads default if not specified)
    #[arg(long)]
    model: Option<PathBuf>,

    /// Launch GUI
    #[arg(long)]
    gui: bool,
}

fn auto_locate_ort_dylib() {
    if std::env::var("ORT_DYLIB_PATH").is_ok() {
        return;
    }

    let exe_path = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new("."));
    let native_name = if cfg!(target_os = "windows") {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };

    // Extract embedded DLL if it doesn't exist next to the exe yet
    #[cfg(all(target_os = "windows", embed_ort_dll))]
    {
        let dll_path = exe_dir.join(native_name);
        if !dll_path.exists() {
            if let Some(parent) = dll_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&dll_path, EMBEDDED_ORT_DLL).is_ok() {
                if let Some(path_str) = dll_path.to_str() {
                    std::env::set_var("ORT_DYLIB_PATH", path_str);
                    return;
                }
            }
        } else {
            if let Some(path_str) = dll_path.to_str() {
                std::env::set_var("ORT_DYLIB_PATH", path_str);
                return;
            }
        }
    }

    let candidates: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![
            // .app bundle: Contents/MacOS/../Frameworks/libonnxruntime.dylib
            exe_dir.join("../Frameworks/libonnxruntime.dylib"),
            // Same directory as binary
            exe_dir.join("libonnxruntime.dylib"),
            // Homebrew (Apple Silicon)
            PathBuf::from("/opt/homebrew/lib/libonnxruntime.dylib"),
            // Homebrew (Intel)
            PathBuf::from("/usr/local/lib/libonnxruntime.dylib"),
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            exe_dir.join("onnxruntime.dll"),
        ]
    } else {
        vec![
            exe_dir.join("libonnxruntime.so"),
            PathBuf::from("/usr/lib/libonnxruntime.so"),
            PathBuf::from("/usr/local/lib/libonnxruntime.so"),
        ]
    };

    for candidate in &candidates {
        if candidate.exists() {
            if let Some(path_str) = candidate.to_str() {
                std::env::set_var("ORT_DYLIB_PATH", path_str);
                return;
            }
        }
    }
}

fn main() {
    auto_locate_ort_dylib();
    let args = Args::parse();

    if args.gui || args.input.is_none() {
        #[cfg(feature = "gui")]
        {
            gui::run();
            return;
        }
        #[cfg(not(feature = "gui"))]
        {
            eprintln!("GUI support is not compiled in. Build with --features gui to enable it.");
            eprintln!("Usage: audio-separator <INPUT> [OPTIONS]");
            std::process::exit(1);
        }
    }

    let input = args.input.as_ref().unwrap();
    if !input.exists() {
        eprintln!("Error: input file does not exist: {}", input.display());
        std::process::exit(1);
    }

    let output_dir = args
        .output
        .unwrap_or_else(|| input.parent().unwrap_or(std::path::Path::new(".")).to_path_buf());

    let (progress_tx, progress_rx) = mpsc::channel();

    let mut last_stage = String::new();
    std::thread::spawn(move || {
        while let Ok(update) = progress_rx.recv() {
            let update: audio_separator::ProgressUpdate = update;
            if update.stage != last_stage {
                last_stage = update.stage.clone();
                eprint!("\n");
            }
            let label = match update.stage.as_str() {
                "downloading_model" => "Downloading model",
                "decoding" => "Decoding audio",
                "resampling" => "Resampling",
                "separating" => "Separating",
                "resampling_output" => "Resampling output",
                "writing" => "Writing output",
                _ => &update.stage,
            };
            eprint!("\r{}: {:.0}%", label, update.progress * 100.0);
        }
        eprint!("\n");
    });

    match audio_separator::separate_file(
        input,
        &output_dir,
        args.model.as_deref(),
        Some(progress_tx),
    ) {
        Ok(result) => {
            println!("Separation complete!");
            println!("  Vocals: {}", result.vocals_path.display());
            println!(
                "  Accompaniment: {}",
                result.accompaniment_path.display()
            );
            println!(
                "  Duration: {:.1}s, Sample rate: {} Hz",
                result.duration_secs, result.sample_rate
            );
        }
        Err(e) => {
            eprintln!("\nError: {}", e);
            std::process::exit(1);
        }
    }
}
