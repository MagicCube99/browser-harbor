# 更新记录

[English](CHANGELOG.md) · 简体中文

## v0.2.0-beta.2 — 2026-10-26

### 调整

- 签名发布清单不再设置实际操作有效期。
- 客户端对有效签名清单最多每天刷新一次，而不是一直复用到期日。
- 元数据工作流只在上游发布信息变化时重新生成并签名。

## v0.2.0-beta.1 — 2026-10-26

验证版 Beta。

### 新增

- 使用 Ed25519 签名发布元数据，公钥内嵌于 EXE。
- 内置签名清单覆盖全部 19 个目标/渠道/CPU 组合，以及 Chrome++ 与 Libportable。
- 可通过已验证的镜像刷新签名清单，并在失败时回退到可信缓存；界面明确显示元数据来源。
- 提供密钥生成、清单签名与验证的维护工具。

### 安全

- GitHub 官方元数据不再经过第三方加速站。
- 浏览器、Chrome++ 和 Libportable 安装必须具有非空完整性值。
- 本地安装包执行相同的可信摘要校验。
- Chrome/Edge 正常解析不再回退到 Bush2021 元数据。

### 兼容性

- `v0.1.0` 保留为冻结的“元数据未签名”兼容版。
- `manager.json` 继续使用 schema v10，可信元数据单独缓存。

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
