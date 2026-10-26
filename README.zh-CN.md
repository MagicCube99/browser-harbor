<p align="center">
  <img src="assets/app-icon.png" width="112" height="112" alt="Browser Harbor 程序图标">
</p>

<h1 align="center">Browser Harbor（浏览器港湾）</h1>

<p align="center">
  <b>面向 Windows 的便携浏览器下载与管理工具</b><br>
  下载、校验、安装、更新和配置 14 个浏览器目标。
</p>

<p align="center">
  <a href="https://github.com/MagicCube99/browser-harbor/releases"><img src="https://img.shields.io/badge/Download-Releases-blue" alt="下载发布版本"></a>
  <img src="https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4" alt="Windows 10 | 11">
  <img src="https://img.shields.io/badge/x64-64--bit-blue" alt="x64">
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green" alt="MIT OR Apache-2.0"></a>
</p>

<p align="center">
  <a href="README.md">English</a> ·
  <a href="docs/USER_GUIDE.zh-CN.md">用户指南</a> ·
  <a href="CHANGELOG.zh-CN.md">更新记录</a>
</p>

Browser Harbor 是一个小型原生 Windows 工具，用于下载浏览器安装包、校验文件、构建便携目录，并在升级时保留用户数据。程序和用户数据分目录管理，Profile 始终由用户掌控。

它不启动浏览器、不注册默认浏览器、不固定任务栏、不监管浏览器进程，也不读取浏览器历史。

## 下载

从 [GitHub Releases](https://github.com/MagicCube99/browser-harbor/releases/latest) 下载 `BrowserHarbor-0.2.1-x64.exe`。

把 EXE 放到任意可写目录后直接运行。无需安装、管理员权限、后台服务或 WebView2。

运行要求：Windows 10 22H2 或 Windows 11 x64。

普通使用只需下载 EXE。Release 中另附的清单、签名和公钥仅用于独立验证，可选下载。

## 功能

- 下载、校验、安装、更新和重新定位受支持的便携浏览器。
- 将浏览器程序、数据、Profile 和可选缓存目录分开保存。
- 受支持的 Chromium 目标提供完整 Chrome++；官方 Firefox 与 Zen 强制使用 Libportable。
- 只读配置检查和安全路径修复。
- 在每个 Browser Root 自动创建快捷方式。
- 立即自动保存、跟随系统/浅色/深色外观、中英文界面。

## 支持浏览器

| 系列 | 浏览器 |
|---|---|
| Chromium | Brave、Catsxp、Chrome、Edge、Helium、Thorium、Turbo、Ungoogled Chromium |
| Firefox | Firefox、Floorp、Iceweasel、LibreWolf、SkyKakapo、Zen |

Iceweasel 提供 ESR 与 Release 渠道；Thorium 提供 SSE3、SSE4、AVX、AVX2、AVX512 构建。

## 当前限制

- Brave、Helium、Ungoogled Chromium 的 Chrome++ 兼容性仍标为实验性。
- EXE 尚未做 Authenticode 代码签名，Windows SmartScreen 可能显示警告。
- 不会自动导入任意已有浏览器目录；请选择目标后使用“检查配置”或“重新定位…”。
- 默认浏览器注册、任务栏集成、浏览器启动、进程监管和历史读取不在范围内。

## 校验下载

请与 Release 说明中的 SHA-256 核对 EXE。浏览器包和运行时组件会在安装前自动校验；加速站只改变传输路径，不能改变预期版本、哈希、签名或布局。

## 文档

- [用户指南](docs/USER_GUIDE.zh-CN.md) / [English](docs/USER_GUIDE.md)
- [更新记录](CHANGELOG.zh-CN.md) / [Changelog](CHANGELOG.md)
- [安全说明](SECURITY.zh-CN.md) / [隐私说明](PRIVACY.zh-CN.md)
- [第三方声明](THIRD_PARTY_NOTICES.md)
- [参与贡献](CONTRIBUTING.zh-CN.md)

## 许可证

项目自身代码采用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 双许可证，SPDX 标识为 `MIT OR Apache-2.0`，下游可以任选其一。

第三方组件和下载的浏览器包适用各自许可证。Browser Harbor 与浏览器厂商不存在隶属或背书关系。
