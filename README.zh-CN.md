<p align="center">
  <img src="assets/app-icon.png" width="112" height="112" alt="Browser Harbor 程序图标">
</p>

<h1 align="center">Browser Harbor（浏览器港湾）</h1>

<p align="center">
  <b>面向 Windows 的便携浏览器下载与管理工具</b><br>
  下载、校验、安装、更新和配置受支持浏览器，不需要安装常驻管理器。
</p>

<p align="center">
  <a href="https://github.com/MagicCube99/browser-harbor/releases/latest"><img src="https://img.shields.io/github/v/release/MagicCube99/browser-harbor?label=%E4%B8%8B%E8%BD%BD%20Download" alt="下载"></a>
  <img src="https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4" alt="Windows 10 | 11">
  <img src="https://img.shields.io/badge/x64-64--bit-blue" alt="x64">
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green" alt="MIT OR Apache-2.0"></a>
</p>

<p align="center">
  <a href="README.md">English</a> ·
  <a href="docs/USER_GUIDE.zh-CN.md">用户指南</a> ·
  <a href="CHANGELOG.zh-CN.md">更新记录</a>
</p>

Browser Harbor 是一个精简的原生 Windows 工具，支持 14 个 Chromium 系和 Firefox 系浏览器目标。它从已声明的上游来源下载浏览器，校验制品，构建便携目录，保留用户数据，修复运行时路径，并把程序、数据、Profile 和缓存分开管理。

它不启动浏览器、不注册默认浏览器、不监管浏览器进程、不固定任务栏，也不读取浏览器历史。

## 下载

从 [v0.2.0-beta.2 Release](https://github.com/MagicCube99/browser-harbor/releases/tag/v0.2.0-beta.2) 下载 `BrowserHarbor-0.2.0-beta.2-x64.exe`。

把 EXE 放到任意可写目录后直接运行。无需安装、管理员权限、后台服务或 WebView2。

运行要求：Windows 10 22H2 或 Windows 11 x64。

`v0.1.0` 保留为冻结的兼容版；`v0.2.x` 是验证版，增加签名发布清单，但不改变浏览器大文件原有的镜像下载路径。

## 功能亮点

- 一个原生 EXE 管理 14 个浏览器目标。
- 使用 Ed25519 验证发布清单，强制校验制品 SHA-256/SHA-512；LibreWolf 额外验证官方 OpenPGP 签名。
- 受控解压、暂存安装、原子替换和失败回滚。
- 独立保存 Browser Root、程序、数据、Profile 和可选缓存路径。
- 适用于目标时提供完整 Chrome++，官方 Firefox 与 Zen 强制 Libportable。
- 只读配置检查、运行时 INI 修复和浏览器身份校验。
- Edge 版本目录自动解析和单实例保护。
- 立即自动保存、跟随系统浅色/深色外观、固定紧凑界面。
- 中英文界面切换，并在重启后保持语言偏好。

## 支持目标

| 系列 | 浏览器 |
|---|---|
| Chromium | Brave、Catsxp、Chrome、Edge、Helium、Thorium、Turbo、Ungoogled Chromium |
| Firefox | Firefox、Floorp、Iceweasel、LibreWolf、SkyKakapo、Zen |

Brave、Helium、Ungoogled Chromium 仍带有实验性 Chrome++ 兼容标记。Iceweasel 提供 ESR 与 Release 渠道；Thorium 提供 SSE3、SSE4、AVX、AVX2、AVX512 构建。

## 安全边界

- 不直接执行下载的浏览器安装器。
- 官方 API 或项目签名清单确定发布身份；镜像和加速站只负责传输字节。
- EXE 内置签名清单覆盖全部 19 个渠道/CPU 组合和 Chrome++、Libportable，元数据站点不可用时仍可启动安装。
- 定时的元数据工作流会自动刷新并签名上游变化，不需要发布新 EXE。
- 程序替换按目标规则保留用户数据和运行时配置。
- EXE 尚未做 Authenticode 代码签名，Windows SmartScreen 可能提示警告。

详见 [SECURITY.zh-CN.md](SECURITY.zh-CN.md)、[PRIVACY.zh-CN.md](PRIVACY.zh-CN.md) 和 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

## 文档

- [用户指南](docs/USER_GUIDE.zh-CN.md) / [English](docs/USER_GUIDE.md)
- [更新记录](CHANGELOG.zh-CN.md) / [Changelog](CHANGELOG.md)
- [架构说明](docs/ARCHITECTURE.md)
- [参与贡献](CONTRIBUTING.zh-CN.md)

## 从源码构建

前置条件：Rust 1.99.1，以及安装“使用 C++ 的桌面开发”工作负载的 Visual Studio Build Tools。

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo build --release
```

或运行完整的 Windows 质量门禁：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
```

## 许可证

项目自身代码采用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 双许可证，SPDX 标识为 `MIT OR Apache-2.0`，下游可以任选其一。

第三方组件和下载的浏览器包适用各自许可证。Browser Harbor 与浏览器厂商不存在隶属或背书关系。
