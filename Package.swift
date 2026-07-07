// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "AuditaurApple",
    platforms: [
        .iOS(.v13),
        .macOS(.v12),
        .tvOS(.v13),
        .watchOS(.v6),
    ],
    products: [
        .library(
            name: "AuditaurAppleCore",
            targets: ["AuditaurAppleCore"]
        ),
    ],
    targets: [
        .target(
            name: "AuditaurAppleCore",
            path: "swift/Sources/AuditaurAppleCore"
        ),
        .testTarget(
            name: "AuditaurAppleCoreTests",
            dependencies: ["AuditaurAppleCore"],
            path: "swift/Tests/AuditaurAppleCoreTests"
        ),
    ]
)
