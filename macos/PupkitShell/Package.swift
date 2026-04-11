// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "PupkitShell",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "PupkitShell", targets: ["PupkitShell"]),
    ],
    targets: [
        .executableTarget(
            name: "PupkitShell",
            path: "Sources/PupkitShell",
            swiftSettings: [.unsafeFlags(["-parse-as-library"])]
        )
    ]
)
