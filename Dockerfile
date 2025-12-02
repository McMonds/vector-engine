FROM rust:1.74-slim-bookworm

# Install Python for simple HTTP server and build essentials
RUN apt-get update && apt-get install -y python3 build-essential && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifest and source
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY viz.html ./

# Build release binaries
RUN cargo build --release

# Expose port for visualization (8000) and API (8080)
EXPOSE 8000
EXPOSE 8080

# Copy entrypoint script
COPY docker-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

ENTRYPOINT ["docker-entrypoint.sh"]
