# 更新记录

[English](CHANGELOG.md) · 简体中文

## v0.1.0 — 2026-10-26

Browser Harbor 首个稳定版本。

### 新增

- 一个原生 Windows x64 便携 EXE，支持 14 个 Chromium 系和 Firefox 系浏览器目标。
- 下载校验、受控解压、暂存安装、原子替换和失败回滚。
- 独立保存 Browser Root、程序、数据、Profile 和可选缓存路径。
- 支持目标使用完整 Chrome++；官方 Firefox 与 Zen 强制 Libportable。
- 只读配置检查、安全运行时路径修复、重新定位预览和浏览器身份校验。
- 立即自动保存、跟随系统/浅色/深色外观、固定紧凑界面、窗口位置恢复和单实例保护。
- 中英文界面切换，并持久保存语言偏好。
- Edge 版本目录自动解析。
- 中英文用户、安全、隐私、贡献和发布文档。

### 支持目标

- Chromium：Brave、Catsxp、Chrome、Edge、Helium、Thorium、Turbo、Ungoogled Chromium。
- Firefox：Firefox、Floorp、Iceweasel、LibreWolf、SkyKakapo、Zen。

### 已知限制

- Brave、Helium、Ungoogled Chromium 仍为实验性 Chrome++ 兼容。
- EXE 尚未做 Authenticode 代码签名。
- Browser Root 移动后需要显式执行运行时 INI 修复，不会自动改写。
- 尚未实现从现有浏览器目录自动识别和导入。
