use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    tauri_build::build();

    #[cfg(target_os = "macos")]
    {
        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let src = manifest.join("macos/wechat_ocr.swift");
        let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("deskcap-wechat-ocr");
        println!("cargo:rerun-if-changed={}", src.display());
        let status = Command::new("swiftc")
            .args(["-O", "-o"])
            .arg(&out)
            .arg(&src)
            .status();
        match status {
            Ok(s) if s.success() => {
                // Copy next to the final binary location used by tauri bundle.
                let dest_dir = manifest.join("target/macos-helpers");
                let _ = std::fs::create_dir_all(&dest_dir);
                let dest = dest_dir.join("deskcap-wechat-ocr");
                let _ = std::fs::copy(&out, &dest);
                println!("cargo:rustc-env=DESKCAP_WECHAT_OCR={}", dest.display());
            }
            Ok(s) => println!("cargo:warning=wechat_ocr.swift compile failed: {s}"),
            Err(e) => println!("cargo:warning=swiftc missing for wechat OCR helper: {e}"),
        }
    }
}
