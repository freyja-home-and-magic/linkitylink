// swift-tools-version:5.5
import PackageDescription

let package = Package(
    name: "tauri-plugin-identity-keychain",
    platforms: [
        .iOS(.v13)
    ],
    products: [
        .library(
            name: "tauri-plugin-identity-keychain",
            type: .static,
            targets: ["tauri-plugin-identity-keychain"])
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api"),
        .package(url: "https://github.com/Brendonovich/swift-rs", branch: "main")
    ],
    targets: [
        .target(
            name: "tauri-plugin-identity-keychain",
            dependencies: [
                .product(name: "SwiftRs", package: "swift-rs"),
                .byName(name: "Tauri")
            ],
            path: "Sources")
    ]
)
