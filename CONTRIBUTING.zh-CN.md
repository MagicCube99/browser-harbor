# 参与贡献

[English](CONTRIBUTING.md) · 简体中文

## 开发环境

- Windows 10/11 x64。
- Rust 1.99.1。
- 安装“使用 C++ 的桌面开发”工作负载的 Visual Studio Build Tools。
- Git 和 PowerShell。

## 构建与测试

在仓库根目录运行：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo build --release
```

运行完整的 Windows 质量门禁：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
```

真实网络测试会下载大型浏览器包，因而默认关闭：

```powershell
cargo test --test providers_live -- --ignored --nocapture
cargo test --test live_artifacts -- --ignored --nocapture --test-threads=1
cargo test --test live_install -- --ignored --nocapture --test-threads=1
```

## Pull Request

- 每个 Pull Request 只处理一个逻辑变更。
- 解析器、安装事务、路径处理和信任边界需要回归测试。
- 保持 `MIT OR Apache-2.0` 双许可。
- 新增或修改运行时组件及许可证时同步更新 `THIRD_PARTY_NOTICES.md`。
- 不提交下载的浏览器、生成的 EXE、缓存、秘密或个人绝对路径。

## 浏览器支持声明

不能仅凭编译或解压成功就移除实验性 Chrome++ 标记。必须验证真实浏览器的数据落点、登录保持、升级保留和配置修复行为。
