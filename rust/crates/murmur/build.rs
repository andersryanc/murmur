// On macOS sherpa-onnx is linked as shared libraries: next to the binary during development
// (its build script copies them there), and in Contents/Frameworks inside Murmur.app.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path");
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path/../Frameworks");
    }
}
