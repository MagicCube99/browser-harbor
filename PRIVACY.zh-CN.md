# 隐私说明

[English](PRIVACY.md) · 简体中文

Browser Harbor 是本地 Windows 工具，不包含遥测、分析、账号系统、广告 SDK 或远程用户配置服务。

## 本地数据

程序会保存：

- 管理器设置在 `%LOCALAPPDATA%\browser-harbor\BrowserHarbor.json`；
- 可重建下载缓存在 `%TEMP%\browser-harbor\cache`；
- 浏览器程序与用户数据只保存在用户选择的 Browser Root 和路径覆盖中。

Browser Harbor 不会收集浏览器历史、Cookie、凭据或 Profile 内容用于上传。

## 网络请求

联网仅用于：

- 查询浏览器、Chrome++ 与 Libportable 发布元数据；
- 通过直连或加速传输刷新 Ed25519 签名的 Browser Harbor 发布清单；
- 下载浏览器包和经过校验的运行时；
- 通过选定传输路由访问 GitHub 与厂商官方接口。

服务提供方必然能看到常规网络元数据，例如 IP 地址、TLS 连接信息、请求 URL 和 User-Agent。Browser Harbor 不会在这些请求中加入持久安装标识。

官方元数据只使用直连或用户代理；签名清单会缓存到可重建下载缓存中，启用内置或自定义加速站时可能通过对应站点下载。

## 日志与诊断

Browser Harbor 不创建常驻应用日志。配置检查只在界面中显示本地路径与错误；公开求助前请删除个人信息和绝对路径。签名元数据缓存只包含发布信息，不包含浏览器用户数据。

## 删除数据

删除 `%LOCALAPPDATA%\browser-harbor\BrowserHarbor.json` 可清除管理器设置。若要同时删除浏览器程序与用户数据，请手工删除对应 Browser Root。
