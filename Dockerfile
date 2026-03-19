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

# 先复制 Cargo 配置以利用依赖缓存
COPY Cargo.toml Cargo.lock ./

# 逐个复制 crate 的 Cargo.toml（保持目录结构）
COPY crates/cogkos-core/Cargo.toml crates/cogkos-core/Cargo.toml
COPY crates/cogkos-store/Cargo.toml crates/cogkos-store/Cargo.toml
COPY crates/cogkos-mcp/Cargo.toml crates/cogkos-mcp/Cargo.toml
COPY crates/cogkos-ingest/Cargo.toml crates/cogkos-ingest/Cargo.toml
COPY crates/cogkos-sleep/Cargo.toml crates/cogkos-sleep/Cargo.toml
COPY crates/cogkos-llm/Cargo.toml crates/cogkos-llm/Cargo.toml
COPY crates/cogkos-federation/Cargo.toml crates/cogkos-federation/Cargo.toml
COPY crates/cogkos-external/Cargo.toml crates/cogkos-external/Cargo.toml
COPY crates/cogkos-workflow/Cargo.toml crates/cogkos-workflow/Cargo.toml

# 创建虚拟源文件以缓存依赖构建
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && \
    for crate in cogkos-core cogkos-store cogkos-mcp cogkos-ingest cogkos-sleep \
                 cogkos-llm cogkos-federation cogkos-external cogkos-workflow; do \
        mkdir -p "crates/$crate/src" && echo '' > "crates/$crate/src/lib.rs"; \
    done && \
    cargo build --release 2>/dev/null || true

# 复制实际源代码和迁移文件
COPY . .

# 触发完整重编译（虚拟源文件的时间戳已过期）
RUN touch src/main.rs && \
    for crate in cogkos-core cogkos-store cogkos-mcp cogkos-ingest cogkos-sleep \
                 cogkos-llm cogkos-federation cogkos-external cogkos-workflow; do \
        touch "crates/$crate/src/lib.rs"; \
    done

# 构建发布版本
RUN cargo build --release --bin cogkos --bin cogkos-admin

# Runtime stage - Distroless
FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /app

# 复制二进制和迁移文件
COPY --from=builder /app/target/release/cogkos /app/cogkos
COPY --from=builder /app/target/release/cogkos-admin /app/cogkos-admin
COPY --from=builder /app/migrations /app/migrations

USER nonroot:nonroot

EXPOSE 3000 8081

ENTRYPOINT ["/app/cogkos"]
