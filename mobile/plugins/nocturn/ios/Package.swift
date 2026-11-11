// swift-tools-version:5.9

import PackageDescription

let package = Package(
    name: "tauri-plugin-nocturn",
    platforms: [
        .iOS("26.0"),
        .macOS(.v13),
    ],
    products: [
        .library(
            name: "tauri-plugin-nocturn",
            type: .static,
            targets: ["tauri-plugin-nocturn"])
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "tauri-plugin-nocturn",
            dependencies: [
                .byName(name: "Tauri")
            ],
            path: "Sources")
    ]
)
