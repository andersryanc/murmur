// On macOS sherpa-onnx is linked as shared libraries that its build script copies next to each
// binary, but its rpath flags only apply to its own crate, so the examples set their own.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-examples=-Wl,-rpath,@executable_path");
        println!("cargo:rustc-link-arg-examples=-Wl,-rpath,@executable_path/..");
    }
}
