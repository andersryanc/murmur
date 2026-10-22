// On macOS sherpa-onnx is linked as shared libraries, which its build script copies next to the
// binary in target/; its own rpath flags only apply to its crate, so set one here.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path");
    }
}
