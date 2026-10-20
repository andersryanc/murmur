// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "MurmurEval",
    platforms: [.macOS(.v26)],
    dependencies: [
        .package(url: "https://github.com/FluidInference/FluidAudio.git", from: "0.17.7"),
    ],
    targets: [
        .executableTarget(
            name: "MurmurEval",
            dependencies: [.product(name: "FluidAudio", package: "FluidAudio")],
            linkerSettings: [
                // Embed an Info.plist so macOS shows the mic / speech permission prompts for this CLI.
                .unsafeFlags(["-Xlinker", "-sectcreate", "-Xlinker", "__TEXT", "-Xlinker", "__info_plist",
                              "-Xlinker", "Support/Info.plist"]),
            ]
        ),
    ]
)
