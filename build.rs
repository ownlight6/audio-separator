use std::path::Path;

fn main() {
    // --- Embed ONNX model ---
    println!("cargo:rustc-check-cfg=cfg(embed_model)");
    let model_path = Path::new("Kim_Vocal_1.onnx");
    if model_path.exists() {
        println!("cargo:rustc-cfg=embed_model");
        println!("cargo:warning=Embedding Kim_Vocal_1.onnx into binary ({} bytes)",
            std::fs::metadata(model_path).map(|m| m.len()).unwrap_or(0));
    } else {
        println!("cargo:warning=Kim_Vocal_1.onnx not found, model will be downloaded at runtime");
    }
    println!("cargo:rerun-if-changed=Kim_Vocal_1.onnx");

    // --- Embed onnxruntime DLL (Windows only) ---
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-check-cfg=cfg(embed_ort_dll)");
        let dll_path = Path::new("onnxruntime.dll");
        if dll_path.exists() {
            println!("cargo:rustc-cfg=embed_ort_dll");
            println!("cargo:warning=Embedding onnxruntime.dll into binary ({} bytes)",
                std::fs::metadata(dll_path).map(|m| m.len()).unwrap_or(0));
        } else {
            println!("cargo:warning=onnxruntime.dll not found; will search system paths at runtime");
        }
        println!("cargo:rerun-if-changed=onnxruntime.dll");
    }
}
