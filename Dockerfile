# Build stage
FROM rust:1.81 AS builder
WORKDIR /usr/src/noah

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --features full
RUN rm -rf src

# Copy source code and build
COPY src ./src
RUN cargo build --release --features full
# Install flamegraph for profiling (optional, only if profiling is enabled)
RUN cargo install flamegraph

# Runtime stage
FROM debian:bookworm-slim
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary from the builder stage
COPY --from=builder /usr/src/noah/target/release/noah /app/noah
# Copy flamegraph binary (optional)
COPY --from=builder /usr/local/cargo/bin/flamegraph /usr/local/bin/flamegraph

# Set environment variables
ENV RUST_LOG=info
ENV DB_TYPE=clickhouse

# Expose ports for HTTP API and metrics
EXPOSE 8081
EXPOSE 9090

# Set the entrypoint
ENTRYPOINT ["/app/noah"]
# Default command (overridable via docker-compose or CLI)
CMD ["--command", "launch-flood"]