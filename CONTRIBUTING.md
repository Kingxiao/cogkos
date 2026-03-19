# CogKOS 开发贡献指南

## 开发环境设置

1. **安装Rust 1.94+**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup update
   ```

2. **安装依赖工具**
   ```bash
   cargo install sqlx-cli --no-default-features --features postgres
   cargo install cargo-deny cargo-audit cargo-tarpaulin
   ```

3. **启动开发环境**
   ```bash
   cp .env.example .env
   # Edit .env — at minimum DATABASE_URL must match your docker-compose ports
   docker-compose up -d
   # Migrations run automatically on first server start
   ```

## 代码提交规范

- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码
- 所有提交必须通过CI检查
- 提交信息使用中文或英文，格式：`<类型>: <描述>`

## 类型标签

- `feat`: 新功能
- `fix`: 修复
- `docs`: 文档
- `refactor`: 重构
- `test`: 测试
- `chore`: 构建/工具

## 分支策略

- `main`: 生产分支
- `develop`: 开发分支
- `feature/*`: 功能分支
- `fix/*`: 修复分支
