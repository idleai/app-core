// swift-tools-version: 6.0
// Staged in dist/native by build-bindings.sh.
import Foundation
import PackageDescription

let libraries = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
let package = Package(
    name: "AppCoreSwiftSmoke",
    dependencies: [.package(path: "../types/swift/AppTypes")],
    targets: [
        .systemLibrary(name: "AppCoreBindingsFFI", path: "swift"),
        .target(
            name: "AppCoreBindings",
            dependencies: ["AppCoreBindingsFFI"],
            path: "swift",
            exclude: ["boltffi.h", "module.modulemap"]
        ),
        .executableTarget(
            name: "SwiftSmoke",
            dependencies: [
                "AppCoreBindings",
                .product(name: "AppTypes", package: "AppTypes"),
            ],
            path: "swift-smoke",
            linkerSettings: [.unsafeFlags([
                "-L", libraries, "-Xlinker", "-rpath", "-Xlinker", libraries,
            ])]
        ),
    ]
)
