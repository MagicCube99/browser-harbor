# Browser Harbor 用户指南

[English](USER_GUIDE.md) · 简体中文

## 运行要求

- Windows 10 22H2 或 Windows 11 x64。
- Browser Harbor EXE 所在目录可写。
- 每个已安装浏览器都有可写的 Browser Root。
- 能访问所选浏览器厂商或经验证的下载来源。

Browser Harbor 是单 EXE 便携程序，不需要安装，也不需要管理员权限。

## 首次启动

1. 从 GitHub Releases 下载 `BrowserHarbor-0.2.1-x64.exe`。
2. 放到可写目录后直接运行。
3. 选择浏览器。
4. 检查程序、数据、Profile 和缓存目录。
5. 若要连接已有的便携安装，请先运行“检查配置”。

每次有效编辑都会立即保存，没有“保存”或“取消”按钮。

管理器配置位于：

```text
%LOCALAPPDATA%\browser-harbor\manager.json
```

使用底栏的 `中` / `EN` 控件切换语言，选择会立即保存。

## 下载与更新

打开浏览器页后可以使用：

- **检查更新**：刷新上游发布信息。
- **下载并安装/升级**：下载、校验并安装当前目标。
- **重新下载并替换**：上游版本相同时重新替换当前程序。
- **载入本地包**：使用已下载的官方安装包。

Browser Harbor 会在修改安装前检查浏览器版本和下载完整性。同版本替换仍然可用，但需要确认。

进入浏览器页后，程序会对当前目标、渠道和构建自动检查一次；也可以手动刷新。十分钟内开始安装时会复用已验证的上游结果。

## 目录

每个 Browser Target 独立保存：

- Browser Root
- 程序目录
- 数据目录
- Profile 目录
- 可选缓存目录

修改 Browser Root 会重新计算标准子目录。运行时支持时，可以单独覆盖某个目录。

`.\Helium` 这样的输入按 Browser Harbor EXE 所在目录解析；`manager.json` 保存解析后的绝对路径。浏览器运行时的程序与数据位于同一磁盘时，会优先使用相对路径。

## 检查与修复

“检查配置”完全只读，检查：

- 必需目录和真实 x64 浏览器可执行文件；
- 可识别时的浏览器身份；
- 本地版本是否可读取；
- 必需便携运行时文件；
- Edge 的 `version.dll` 导入状态；
- Chrome++、Libportable、Iceweasel 和 SkyKakapo 的路径配置。

Chrome++ 仍保存旧绝对路径时，可以在确认后修复。修复前会要求关闭浏览器并备份当前 INI。

### 浏览器目录移动后重新定位

1. 打开“检查配置”。
2. 点击“重新定位…”。
3. 选择新的 Browser Root。
4. 检查即将应用的路径和身份提示。
5. 点击“应用”。

旧 Browser Root 内的路径会按相对位置迁移，旧根目录外的自定义路径保持不变。重新定位只更新管理器配置，不会自动改写浏览器 INI。

## Chrome++

Chrome++ 支持 Brave、Chrome、Edge、Helium、Thorium 和 Ungoogled Chromium。Chrome++ 页面显示已安装版本、源码/GPL 许可证入口和全部公开配置。

Edge 会使用同版本 `setdll` 添加 `version.dll` 导入。Brave、Helium 和 Ungoogled Chromium 仍为实验性兼容。

## Firefox 系运行时

- 官方 Firefox 和 Zen 强制使用 Libportable。
- Iceweasel 使用包内便携运行时。
- SkyKakapo 使用自带 `tmemutil` 运行时。
- Floorp 和 LibreWolf 保持厂商便携布局。

## 全局设置

底栏设置页控制内置和自定义下载加速站、代理与超时、外观以及下载缓存清理。

加速站只改变传输路径。“清理下载缓存”只删除可重新下载的安装文件，不会删除浏览器 Profile 或已安装程序。

## 常见问题

### 找不到浏览器

运行“检查配置”，确认目标、Browser Root、程序目录和 x64 浏览器 EXE。

### 浏览器目录被移动

使用“重新定位…”选择新的 Browser Root。如果无法确认程序身份，请再次核对目录后再应用。

### Chrome++ 没有生效

确认浏览器 EXE 同目录存在 `version.dll` 和 `chrome++.ini`。Edge 还需要通过诊断中的导入表检查。

### Windows 对 EXE 发出警告

EXE 尚未做 Authenticode 代码签名。请只从 GitHub Releases 获取，并与发布说明中的 SHA-256 核对。

## 卸载

删除 Browser Harbor EXE 和 `%LOCALAPPDATA%\browser-harbor\manager.json`。浏览器及其用户数据仍保留在各自的 Browser Root 中。也可以删除 `%TEMP%\browser-harbor\cache` 和 Browser Root 中的快捷方式。

## 相关文档

- [README](../README.zh-CN.md)
- [更新记录](../CHANGELOG.zh-CN.md)
- [安全说明](../SECURITY.zh-CN.md)
- [隐私说明](../PRIVACY.zh-CN.md)
- [第三方声明](../THIRD_PARTY_NOTICES.md)
