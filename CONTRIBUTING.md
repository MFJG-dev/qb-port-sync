# Contributing to qb-port-sync

Thank you for your interest in contributing to qb-port-sync! This document provides guidelines and instructions for contributing to the project.

## Code Quality Standards

### Formatting

All code must be formatted with `rustfmt` before submission:

```bash
cargo fmt --all
```

To check formatting without making changes:

```bash
cargo fmt --all -- --check
```

### Linting

Code must pass `clippy` with no warnings:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

### Testing

All tests must pass:

```bash
cargo test --all-features --workspace
```

Run tests with output visible:

```bash
cargo test --all-features --workspace -- --nocapture
```

## Building the Project

### Standard Build

```bash
cargo build --release
```

### Build with All Features

This includes PCP support, journald logging, and metrics:

```bash
cargo build --release --all-features
```

### Feature Flags

- `pcp`: Enable PCP (Port Control Protocol) support via `crab_nat`
- `journald`: Enable systemd-journald integration on Linux
- `metrics`: Enable Prometheus metrics and health endpoints

## Docker

### Building the Container

```bash
docker build -t qb-port-sync:latest .
```

### Running the Container

```bash
# Create a config file first
cp config/config.example.toml /path/to/config.toml

# Run with config mounted
docker run --rm \
  -v /path/to/config.toml:/etc/qb-port-sync/config.toml:ro \
  -p 9000:9000 \
  qb-port-sync:latest
```

### Testing the Container

```bash
# Run once mode with JSON output
docker run --rm \
  -v /path/to/config.toml:/etc/qb-port-sync/config.toml:ro \
  qb-port-sync:latest --once --strategy auto --json
```

## Packaging

### Arch Linux (PKGBUILD)

```bash
# Update PKGBUILD version and checksums
makepkg -si
```

### Homebrew Formula

```bash
# Update Formula/qb-port-sync.rb with correct version and SHA256
brew install --build-from-source Formula/qb-port-sync.rb
brew test qb-port-sync
```

### Installation Script

```bash
./install.sh
```

This builds the project and installs the binary, config, and systemd units.

## Submitting Changes

### Before Submitting a PR

1. **Format your code**: `cargo fmt --all`
2. **Run clippy**: `cargo clippy --all-targets --all-features -- -D warnings`
3. **Run tests**: `cargo test --all-features --workspace`
4. **Test your changes**: Verify functionality with `--once` mode or in a test environment
5. **Update documentation**: If you're adding features, update README.md and relevant docs

### Pull Request Process

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Make your changes following the code quality standards above
4. Commit with clear, descriptive messages
5. Push to your fork
6. Open a pull request with:
   - Clear description of changes
   - Motivation/reasoning
   - Any breaking changes noted
   - Test results

### Commit Message Guidelines

- Use present tense ("Add feature" not "Added feature")
- Use imperative mood ("Move cursor to..." not "Moves cursor to...")
- First line should be 50 characters or less
- Reference issues and pull requests when relevant

## Development Tips

### Running in Development

```bash
# Quick test with verbose logging
cargo run -- --once --strategy auto -vv

# Watch mode for rapid iteration
cargo watch -x 'run -- --once --strategy auto'
```

### Debugging

Enable debug logging:

```bash
RUST_LOG=debug cargo run -- --once --strategy auto
```

For trace-level logging:

```bash
RUST_LOG=trace cargo run -- --once --strategy auto -vv
```

### Testing Metrics Endpoint

Enable metrics in your config:

```toml
[metrics]
enabled = true
port = 9000

[health]
enabled = true
port = 9000
```

Then test:

```bash
curl http://localhost:9000/metrics
curl http://localhost:9000/healthz
```

## Architecture Notes

### Key Modules

- `src/main.rs`: CLI, daemon lifecycle, strategy resolution
- `src/config.rs`: Configuration parsing and validation
- `src/qbit.rs`: qBittorrent Web API client
- `src/portmap/`: PCP and NAT-PMP port mapping
- `src/watch.rs`: File watching for ProtonVPN forwarded port
- `src/metrics.rs`: Prometheus metrics and health endpoints (feature-gated)
- `src/report.rs`: JSON output for `--once --json` mode

### Adding New Features

When adding features:

1. Consider whether it should be feature-gated
2. Add appropriate configuration options to `src/config.rs`
3. Update `config/config.example.toml` with examples
4. Add tests where possible
5. Document in README.md

## Questions?

If you have questions about contributing, please open an issue for discussion before starting significant work.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
