# Stage 1: Builder
FROM rust:1.83-bullseye as builder

WORKDIR /usr/src/vector_engine

# Copy manifests first for caching
COPY Cargo.toml Cargo.lock ./

# Create dummy main to cache dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/lib.rs && \
    echo "fn main() {}" > src/main.rs

RUN cargo build --release || true

# Clean dummy build
RUN rm -rf src

# Copy actual source code
COPY src ./src

# Build release binaries
RUN cargo build --release --bins

# Stage 2: Runtime
FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binaries
COPY --from=builder /usr/src/vector_engine/target/release/generator /usr/local/bin/
COPY --from=builder /usr/src/vector_engine/target/release/stress_test /usr/local/bin/

# Default command
CMD ["stress_test", "--help"]
