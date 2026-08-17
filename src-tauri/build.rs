fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        println!(
            "cargo:rustc-link-arg=-Wl,-rpath,/Library/Frameworks/GStreamer.framework/Versions/1.0/lib"
        );
    }

    tauri_build::build()
}
