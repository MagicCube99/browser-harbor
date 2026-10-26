# Browser Harbor 用户指南

[English](USER_GUIDE.md) · 简体中文

## 运行要求

- Windows 10 22H2 或 Windows 11 x64。
- Browser Harbor 目录及每个 Browser Root 需要可写。
- 能访问浏览器厂商、GitHub 和所选运行时来源。

Browser Harbor 是单 EXE 便携程序，不需要安装，也不需要管理员权限。

## 首次启动

1. 从 GitHub Releases 下载 `BrowserHarbor-0.1.0-x64.exe`。
2. 把 EXE 放到可写目录后直接运行。
3. 从浏览器下拉框选择目标。
4. 检查 Browser Root 及其下级路径。
5. 更新已有安装前先点击“检查配置”。

每次有效编辑都会立即保存，没有“保存”或“取消”按钮。

管理器配置位于：

```text
%LOCALAPPDATA%\browser-harbor\manager.json
```

使用底栏的 `中` / `EN` 控件切换界面语言，选择会立即保存。

## 浏览器页

### 浏览器

- **浏览器**：选择 14 个受支持目标之一。
- **渠道**：仅目标存在多产品线时显示。
- **构建**：例如 Thorium 的 SSE3/SSE4/AVX/AVX2/AVX512。
- **当前/上游版本**：分别来自真实浏览器文件与发布元数据。
- **检查更新**：后台检查上游元数据。
- **下载安装/升级/重新下载并替换**：执行经过校验的安装事务。
- **载入本地包**：使用用户选择的离线官方安装包。

即使版本相同，替换入口也不会隐藏；执行前会要求确认。

### 路径与数据

每个目标独立保存：

- Browser Root
- 程序目录
- 数据目录
- Profile 目录
- 可选缓存目录

修改 Browser Root 会按目标真实布局重算子目录。运行时支持时，可单独覆盖 Data、Profile 或缓存路径。

目录输入框接受 `.\...` 相对路径，但保存时会按 EXE 目录解析，并把绝对路径写入 `manager.json`。浏览器自己的运行时配置仍遵循“同盘相对、跨盘绝对”。

### 检查配置

“检查配置”完全只读，检查：

- 根目录、程序、Data、Profile 和缓存路径；
- 真实 x64 浏览器 EXE；
- 本地版本是否可读取；
- 必需便携运行时文件；
- Edge 的 `version.dll` 导入状态；
- Chrome++、Libportable、Iceweasel 和 SkyKakapo 的路径配置。

状态分为 **未安装**、**正常**、**有警告** 和 **有错误**。

如果 Chrome++ 的 `chrome++.ini` 仍保存绝对路径，诊断中会出现“修复 Chrome++ 路径”按钮。确认后程序会先检查浏览器未运行、备份原 INI，再只更新 `data_dir` 和 `cache_dir`；其他 Chrome++ 参数、注释与未知键保持不变。备份名为 `chrome++.ini.path-repair.bak`，重名时依次使用 `.2`、`.3`。

### 搬移目录后重新定位

1. 打开“检查配置”。
2. 点击“重新定位…”。
3. 选择浏览器移动后的 Browser Root。
4. 检查即将写入的路径。
5. 点击“应用”。

旧 Browser Root 内的路径会按相对位置迁移，旧根目录外的自定义路径保持不变。重新定位只更新 `manager.json`，不会自动改写浏览器 INI。

## Chrome++

支持 Brave、Chrome、Edge、Helium、Thorium、Ungoogled Chromium。

- 运行时从 Chrome++ 官方 Release 下载。
- Edge 额外使用同版本 setdll 添加 `version.dll` 导入。
- 配置页开放 Chrome++ 1.18.2 的全部公开 INI 选项。
- 编辑会原子写入 `chrome++.ini`。
- 恢复默认配置前会自动备份。
- Brave、Helium、Ungoogled Chromium 仍为实验性兼容。

## Firefox 系运行时

- 官方 Firefox 和 Zen 强制使用 Libportable。
- Libportable 注入失败即安装失败。
- Iceweasel 使用包内便携运行时。
- SkyKakapo 使用自带 `tmemutil` 运行时。
- Floorp 和 LibreWolf 保持厂商便携布局。

## 全局设置

点击底栏的设置图标后，可以配置：

- 内置下载加速站。
- 用户添加的 HTTPS 自定义加速前缀。
- 最终原始 GitHub 回退时使用用户代理或系统代理。

加速站只改变传输路径，不能改变版本、大小、哈希、签名或布局。

“清理下载缓存”只删除可重建的 `.artifact` 和 `.part`，不会删除浏览器数据、Profile 或发布元数据。

## 便携路径与目录移动

运行时支持时，Browser Harbor 使用可搬迁路径：

- Chrome++：`%app%\..\...`
- Libportable 与 Iceweasel：相对最终程序目录
- SkyKakapo：`PortableDataPath=..\Profiles`

跨磁盘自定义路径会回退绝对路径，并在诊断中提示可搬迁性风险。

这些浏览器运行时配置与 EXE 自身所在磁盘无关。即使管理器 EXE 在 `C:`、浏览器在 `D:`，只要浏览器程序和数据位于同一磁盘且能建立相对关系，运行时配置仍会使用相对表达式；管理器中对应的 Browser Root 则按实际情况保存为绝对路径。

## 常见问题

### 找不到浏览器

运行“检查配置”，确认程序目录中存在正确的 x64 可执行文件，并确认所选目标没有选错。

### 浏览器目录被移动

使用“重新定位”选择新根目录。不要修改其他无关目标。

### Chrome++ 没有生效

确认 EXE 同目录存在 `version.dll` 和 `chrome++.ini`；Edge 还需通过诊断中的导入表检查。

### Firefox 没有使用便携数据目录

确认 Libportable 文件存在，且 `portable.ini` 中的相对 `PortableDataPath` 正确。

### Windows 对 EXE 发出警告

EXE 尚未做代码签名。请只从 GitHub Releases 获取，并与发布说明中的 SHA-256 核对。

## 卸载

删除 Browser Harbor EXE 与 `%LOCALAPPDATA%\browser-harbor\manager.json` 即可。浏览器及其用户数据仍保留在用户选择的 Browser Root 中。

## 相关文档

- [README](../README.zh-CN.md)
- [更新记录](../CHANGELOG.zh-CN.md)
- [安全说明](../SECURITY.zh-CN.md)
- [隐私说明](../PRIVACY.zh-CN.md)
- [第三方声明](../THIRD_PARTY_NOTICES.md)
