use std::path::Path;

fn main() {
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
}
