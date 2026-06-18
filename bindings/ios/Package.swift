// swift-tools-version:5.9
import PackageDescription

// Swift binding over the Uldren Loom C ABI. The C header is vendored at
// Sources/CUldrenLoom/include/loom.h (synced by `just header`); see README.md to build.
let package = Package(
    name: "UldrenLoom",
    // Structured concurrency (`async`/`await`, CheckedContinuation) requires these floors.
    platforms: [.macOS(.v10_15), .iOS(.v13), .tvOS(.v13), .watchOS(.v6)],
    products: [
        .library(name: "UldrenLoom", targets: ["UldrenLoom"]),
    ],
    targets: [
        // C ABI shim: the vendored include/loom.h, exposed as the `CUldrenLoom` module.
        .target(name: "CUldrenLoom"),
        .target(
            name: "UldrenLoom",
            dependencies: ["CUldrenLoom"],
            linkerSettings: [
                .linkedLibrary("uldren_loom"),
                // libuldren_loom search path (from `cargo build --release`); override with -Xlinker -L.
                .unsafeFlags(["-L", "../../target/release"]),
            ]
        ),
        .testTarget(
            name: "UldrenLoomTests",
            dependencies: ["UldrenLoom"],
            path: "Tests/UldrenLoomTests"
        ),
    ]
)
