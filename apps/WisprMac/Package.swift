// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "WisprMac",
    platforms: [
        .macOS(.v13),
    ],
    products: [
        .executable(name: "WisprMacApp", targets: ["WisprMacApp"]),
    ],
    targets: [
        .executableTarget(
            name: "WisprMacApp",
            path: "Sources/WisprMacApp"
        ),
    ]
)
