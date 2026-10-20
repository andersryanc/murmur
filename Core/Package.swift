// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "MurmurCore",
    platforms: [.macOS(.v26)],
    products: [
        .library(name: "MurmurCore", targets: ["MurmurCore"]),
    ],
    dependencies: [
        .package(url: "https://github.com/FluidInference/FluidAudio.git", from: "0.17.7"),
    ],
    targets: [
        .target(
            name: "MurmurCore",
            dependencies: [.product(name: "FluidAudio", package: "FluidAudio")]
        ),
    ]
)
