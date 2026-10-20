// swift-tools-version: 6.2
import PackageDescription

// MurmurKit is plain Foundation and builds on macOS and Linux: the cleanup prompt, the cleaners
// (Claude, Ollama), the personal dictionary, target context and voice activity.
// MurmurCore adds the macOS-only parts (audio capture, speech engines, Apple Intelligence) and is
// only declared on macOS, so Linux never fetches FluidAudio.
var products: [Product] = [
    .library(name: "MurmurKit", targets: ["MurmurKit"]),
]
var dependencies: [Package.Dependency] = []
var targets: [Target] = [
    .target(name: "MurmurKit"),
    .testTarget(name: "MurmurKitTests", dependencies: ["MurmurKit"]),
]

#if os(macOS)
products.append(.library(name: "MurmurCore", targets: ["MurmurCore"]))
dependencies.append(.package(url: "https://github.com/FluidInference/FluidAudio.git", from: "0.17.7"))
targets.append(.target(
    name: "MurmurCore",
    dependencies: ["MurmurKit", .product(name: "FluidAudio", package: "FluidAudio")]
))
#endif

let package = Package(
    name: "MurmurCore",
    platforms: [.macOS(.v26)],
    products: products,
    dependencies: dependencies,
    targets: targets
)
