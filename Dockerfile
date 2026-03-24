# Build stage
FROM rust:1.94-bookworm AS builder

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    libclang-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Copy Cargo configs first to leverage dependency caching
COPY Cargo.toml Cargo.lock ./

# Copy each crate's Cargo.toml (preserve directory structure)
COPY crates/cogkos-core/Cargo.toml crates/cogkos-core/Cargo.toml
COPY crates/cogkos-store/Cargo.toml crates/cogkos-store/Cargo.toml
COPY crates/cogkos-mcp/Cargo.toml crates/cogkos-mcp/Cargo.toml
COPY crates/cogkos-ingest/Cargo.toml crates/cogkos-ingest/Cargo.toml
COPY crates/cogkos-sleep/Cargo.toml crates/cogkos-sleep/Cargo.toml
COPY crates/cogkos-llm/Cargo.toml crates/cogkos-llm/Cargo.toml
COPY crates/cogkos-federation/Cargo.toml crates/cogkos-federation/Cargo.toml
COPY crates/cogkos-external/Cargo.toml crates/cogkos-external/Cargo.toml

# Create dummy source files to cache dependency builds
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && \
    for crate in cogkos-core cogkos-store cogkos-mcp cogkos-ingest cogkos-sleep \
                 cogkos-llm cogkos-federation cogkos-external; do \
        mkdir -p "crates/$crate/src" && echo '' > "crates/$crate/src/lib.rs"; \
    done && \
    cargo build --release 2>/dev/null || true

# Copy actual source code and migration files
COPY . .

# Trigger full rebuild (dummy source timestamps are stale)
RUN touch src/main.rs && \
    for crate in cogkos-core cogkos-store cogkos-mcp cogkos-ingest cogkos-sleep \
                 cogkos-llm cogkos-federation cogkos-external; do \
        touch "crates/$crate/src/lib.rs"; \
    done

# Build release binaries
RUN cargo build --release --bin cogkos --bin cogkos-admin

# Runtime stage - Distroless
FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /app

# Copy binaries and migration files
COPY --from=builder /app/target/release/cogkos /app/cogkos
COPY --from=builder /app/target/release/cogkos-admin /app/cogkos-admin
COPY --from=builder /app/migrations /app/migrations

USER nonroot:nonroot

EXPOSE 3000 8081

ENTRYPOINT ["/app/cogkos"]
