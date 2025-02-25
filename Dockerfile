# Build stage
FROM rust:1.81 AS builder

# Install build dependencies
RUN apt-get update && \
    apt-get install -y \
    libssl-dev \
    pkg-config \
    sqlite3 \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /usr/src/noah

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --features "web-ui"
RUN rm -rf src

# Copy source code and build
COPY src ./src
COPY web ./web
COPY migrations ./migrations
RUN cargo build --release --features "web-ui"

# Install flamegraph for profiling (optional)
RUN cargo install flamegraph

# Runtime stage
FROM debian:bookworm-slim
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y \
    libssl-dev \
    libsqlite3-0 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary from the builder stage
COPY --from=builder /usr/src/noah/target/release/noah /app/noah

# Copy web assets and migrations
COPY --from=builder /usr/src/noah/web /app/web
COPY --from=builder /usr/src/noah/migrations /app/migrations

# Copy flamegraph binary (optional)
COPY --from=builder /usr/local/cargo/bin/flamegraph /usr/local/bin/flamegraph

# Create directory for SQLite database
RUN mkdir -p /app/data && \
    chmod -R 755 /app/data

# Set environment variables
ENV RUST_LOG=info
ENV DATABASE_URL=sqlite:///app/data/noah.db
ENV REDIS_URL=redis://redis:6379
ENV WEBSOCKET_URL=ws://websocket-server:8080/vitals
ENV DB_TYPE=clickhouse

# Expose ports for HTTP API and metrics
EXPOSE 8081 9090

# Set the entrypoint
ENTRYPOINT ["/app/noah"]

# Default command (overridable via docker-compose or CLI)
CMD ["--command", "launch-flood"]