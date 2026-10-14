// swift-tools-version: 5.9
import PackageDescription

// Offline batch harness for the app's local MLX LLM rewrite path.
// Calls the same MLXLLM generation API that MLXBridge/MLXLlmManager uses, so the
// A/B/C test (no-LLM / 0.6B / 1.7B) exercises the exact runtime code path.
// The model is selected by the directory passed at runtime — switch between
// Qwen3-0.6B-4bit and Qwen3-1.7B-4bit without rebuilding.
let package = Package(
    name: "llmbatch",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/ml-explore/mlx-swift.git", from: "0.31.6"),
        .package(url: "https://github.com/ml-explore/mlx-swift-lm.git", from: "3.31.4"),
        .package(url: "https://github.com/huggingface/swift-transformers.git", from: "1.1.6"),
        // Same constraint as mac/Packages/MLXBridge/Package.swift.
        .package(url: "https://github.com/huggingface/swift-jinja.git", "2.0.0"..<"2.4.0"),
    ],
    targets: [
        .executableTarget(
            name: "llmbatch",
            dependencies: [
                .product(name: "MLXLLM", package: "mlx-swift-lm"),
                .product(name: "MLXLMCommon", package: "mlx-swift-lm"),
                .product(name: "MLXHuggingFace", package: "mlx-swift-lm"),
                .product(name: "Tokenizers", package: "swift-transformers"),
                .product(name: "MLXFast", package: "mlx-swift"),
            ]
        ),
    ]
)
