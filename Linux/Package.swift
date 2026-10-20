// swift-tools-version: 6.2
import Foundation
import PackageDescription

// The Linux build of Murmur: a background daemon plus a small CLI that Hyprland key bindings call.
// Speech-to-text is Parakeet through sherpa-onnx's C API. `scripts/setup.sh` downloads the
// sherpa-onnx libraries into `vendor/sherpa-onnx`; set SHERPA_ONNX_DIR to use another copy.
let packageDir = Context.packageDirectory
let sherpa = Context.environment["SHERPA_ONNX_DIR"] ?? "\(packageDir)/vendor/sherpa-onnx"

let package = Package(
    name: "MurmurLinux",
    dependencies: [
        .package(path: "../Core"),
    ],
    targets: [
        .systemLibrary(name: "CSherpaOnnx"),
        .executableTarget(
            name: "murmur",
            dependencies: [
                "CSherpaOnnx",
                .product(name: "MurmurKit", package: "Core"),
            ],
            swiftSettings: [
                .unsafeFlags(["-Xcc", "-I\(sherpa)/include"]),
            ],
            linkerSettings: [
                // Installed: libraries sit in ~/.local/lib/murmur next to ~/.local/bin/murmur.
                // Development (`swift run`): straight from the vendor folder.
                .unsafeFlags(["-L\(sherpa)/lib",
                              "-Xlinker", "-rpath", "-Xlinker", "$ORIGIN/../lib/murmur",
                              "-Xlinker", "-rpath", "-Xlinker", "\(sherpa)/lib"]),
            ]
        ),
    ]
)
