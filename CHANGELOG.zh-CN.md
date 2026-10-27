# 更新记录

[English](CHANGELOG.md) · 简体中文

## v0.3.0 — 2026-10-27

### 调整

- Floorp 和 LibreWolf 改用强制 Libportable 注入，不再依赖发行包内的启动器。
- 六个 Firefox 系目标直接启动浏览器 EXE 时均保持便携。
- Floorp 保留 `data/profile/default`，LibreWolf 保留 `Profiles/Default`。
- 删除 Floorp/LibreWolf 包内启动器及 LibreWolf 更新器、计划任务脚本，避免启动和更新冲突。
- Floorp 本地版本会移除内部的 `@<Gecko 版本>` 后缀，避免与上游产品版本错误显示为不同。

### 可靠性

- 配置 schema v11 只迁移已知旧默认 Profile 路径，保留用户自定义路径。
- Libportable 安装后强制验证 `mozglue.dll` 是否导入 `portable64.dll`。

## v0.2.1 — 2026-10-26

### 修复

- 重新定位时，即使两个浏览器都使用 `chrome.exe`，已知属于其他浏览器的程序也会被拒绝；未知自定义程序会在确认页显示身份警告。
- 保存的窗口位置会限制在当前虚拟桌面内，避免显示器布局变化后程序完全出现在屏幕外。

### 新增

- Chrome++ 页面增加上游源码与 GPL-3.0 许可证入口。

### 文档

- 公开文档聚焦安装、使用、校验和故障排查。

## v0.2.0 — 2026-10-26

- 首个带下载验证的稳定版本。
- 签名发布元数据、自动上游检查和已验证结果复用。
- 浏览器、Chrome++、Libportable 和本地安装包强制执行完整性校验。
- 中文和英文界面。

## v0.1.0 — 2026-10-26

- 首个公开发布，支持 14 个浏览器目标、便携布局、Chrome++/Libportable、配置检查和立即自动保存。

## 当前限制

- Brave、Helium、Ungoogled Chromium 的 Chrome++ 兼容性仍标为实验性。
- EXE 尚未做 Authenticode 代码签名。
- 不会自动导入任意已有浏览器目录。
- 默认浏览器注册、任务栏集成、浏览器启动、进程监管和历史读取不在范围内。
