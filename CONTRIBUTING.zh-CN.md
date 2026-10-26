# 参与贡献

[English](CONTRIBUTING.md) · 简体中文

## 修改代码前

- 使用 Windows 10/11 x64。
- 安装 Rust 1.99.1，以及包含 C++ 桌面开发组件的 Visual Studio Build Tools。
- 阅读 `docs/ARCHITECTURE.md`。
- 保持产品定位为便携浏览器下载与配置管理器；不要加入浏览器启动、默认浏览器注册、任务栏集成或遥测。

## 质量门禁

在仓库根目录运行：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
```

真实网络测试会下载大型制品，必须显式运行：

```powershell
cargo test --test providers_live -- --ignored --nocapture
cargo test --test live_artifacts -- --ignored --nocapture --test-threads=1
cargo test --test live_install -- --ignored --nocapture --test-threads=1
```

## Pull Request

- 每个 Pull Request 只处理一个逻辑变更。
- 解析器、安装事务、路径处理和安全边界必须增加回归测试。
- 开发改动必须在同一变更中更新 `AI_LOG.md`。
- 不提交下载的浏览器、生成的 EXE、缓存、秘密或个人绝对路径。
- 保持 MIT OR Apache-2.0 双许可；新增运行时组件时同步更新 `THIRD_PARTY_NOTICES.md`。

## 浏览器支持声明

不能仅凭编译或解压成功就移除实验性标记。必须取得真实浏览器的数据落点、Cookie/登录跨重启保持、升级保留和清理行为证据。
