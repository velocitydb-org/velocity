# VelocityDB Makefile

.PHONY: build test run clean docker benchmark help

# Default target
help:
	@echo ""
	@echo "Velocity Database Build System"
	@echo ""
	@echo "Available targets:"
	@echo "  build      - Build the project in release mode"
	@echo "  build-linux - Build Linux binary using Docker"
	@echo "  cross-linux - Cross-compile for Linux (x86_64)"
	@echo "  test       - Run all tests"
	@echo "  run        - Run the server with default config"
	@echo "  clean      - Clean build artifacts"
	@echo "  docker     - Build Docker image"
	@echo "  benchmark  - Run performance benchmarks"
	@echo "  format     - Format code with rustfmt"
	@echo "  lint       - Run clippy linter"
	@echo "  docs       - Generate documentation"
	@echo "  install    - Install binary to system"

# Build the project
build:
	@echo "Building Velocity Database..."
	cargo build --release

# Cross-compile for Linux (requires 'cross' crate)
cross-linux:
	@echo "Building for Linux (x86_64)..."
	cross build --target x86_64-unknown-linux-gnu --release

# Build Linux binary using Docker
build-linux:
	@echo "Building Linux binary using Docker..."
	docker build -f Dockerfile.build -t velocitydb-builder .
	@echo "Extracting binary..."
	@if not exist dist mkdir dist
	docker create --name velocitydb-extract velocitydb-builder
	docker cp velocitydb-extract:/velocity ./dist/velocity-linux-x64
	docker rm velocitydb-extract
	@echo "Linux binary created: dist/velocity-linux-x64"

# Run tests
test:
	@echo "Running tests..."
	cargo test

# Run the server
run:
	@echo "Starting Velocity Database server..."
	cargo run -- server --verbose

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	rm -rf ./velocitydb ./test_* ./benchmark_*

# Build Docker image
docker:
	@echo "Building Docker image..."
	docker build -t velocitydb:latest .

# Run Docker container
docker-run:
	@echo "Running Velocity Database in Docker..."
	docker-compose up -d

# Stop Docker container
docker-stop:
	@echo "Stopping Velocity Database Docker containers..."
	docker-compose down

# Run benchmarks
benchmark:
	@echo "Running performance benchmarks..."
	cargo run -- benchmark --operations 100000

# Format code
format:
	@echo "Formatting code..."
	cargo fmt

# Run linter
lint:
	@echo "Running clippy linter..."
	cargo clippy -- -D warnings

# Generate documentation
docs:
	@echo "Generating documentation..."
	cargo doc --open

# Install binary
install:
	@echo "Installing Velocity Database..."
	cargo install --path .

# Create a new user
create-user:
	@echo "Creating new user..."
	@read -p "Username: " username; \
	read -s -p "Password: " password; \
	echo ""; \
	cargo run -- create-user --username $$username --password $$password

# Development setup
dev-setup:
	@echo "Setting up development environment..."
	rustup component add rustfmt clippy
	cargo install cargo-watch
	@echo "Development setup complete!"

# Watch for changes and rebuild
watch:
	@echo "Watching for changes..."
	cargo watch -x "build --release"

# Run integration tests
integration-test:
	@echo "Running integration tests..."
	cargo test --test integration_tests

# Performance profiling
profile:
	@echo "Running performance profiling..."
	cargo build --release
	perf record --call-graph=dwarf ./target/release/velocity benchmark --operations 50000
	perf report

# Memory profiling with valgrind
memory-profile:
	@echo "Running memory profiling..."
	cargo build
	valgrind --tool=massif --stacks=yes ./target/debug/velocity benchmark --operations 10000

# Security audit
audit:
	@echo "Running security audit..."
	cargo audit

# Check for outdated dependencies
outdated:
	@echo "Checking for outdated dependencies..."
	cargo outdated

# Update dependencies
update:
	@echo "Updating dependencies..."
	cargo update

# Full CI pipeline
ci: format lint test audit
	@echo "CI pipeline completed successfully!"

# Release build with optimizations
release: clean
	@echo "Building optimized release..."
	RUSTFLAGS="-C target-cpu=native" cargo build --release
	strip target/release/velocity
	@echo "Release build completed: target/release/velocity"

# Package for distribution
package: release
	@echo "Creating distribution package..."
	mkdir -p dist
	cp target/release/velocity dist/
	cp velocity.toml dist/
	cp README.md dist/
	cp VELOCITY_PROTOCOL.md dist/
	tar -czf dist/velocitydb-$(shell cargo metadata --format-version 1 | jq -r '.packages[0].version')-linux-x64.tar.gz -C dist .
	@echo "Package created: dist/velocitydb-*.tar.gz"