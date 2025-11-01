# Builder stage
FROM rust:1.82-bookworm as builder

WORKDIR /usr/src/qb-port-sync

# Copy manifests
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./

# Copy source
COPY src ./src

# Build with all features
RUN cargo build --release --locked --all-features

# Runtime stage
FROM debian:bookworm-slim

# Install ca-certificates for HTTPS connections
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r qbportsync && \
    useradd -r -g qbportsync -s /bin/false qbportsync

# Create config directory
RUN mkdir -p /etc/qb-port-sync && \
    chown qbportsync:qbportsync /etc/qb-port-sync

# Copy binary from builder
COPY --from=builder /usr/src/qb-port-sync/target/release/qb-port-sync /usr/local/bin/qb-port-sync

# Copy example config
COPY config/config.example.toml /etc/qb-port-sync/config.example.toml

# Copy systemd units for reference
COPY systemd /etc/qb-port-sync/systemd

# Switch to non-root user
USER qbportsync

# Default to config in /etc
ENV RUST_LOG=info

# Expose default metrics/health port
EXPOSE 9000

ENTRYPOINT ["/usr/local/bin/qb-port-sync"]
CMD ["--config", "/etc/qb-port-sync/config.toml"]
