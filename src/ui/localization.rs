use std::cell::Cell;

use eframe::egui::{self, WidgetText};

use crate::domain::settings::Language;

thread_local! {
    static LANGUAGE: Cell<Language> = const { Cell::new(Language::Chinese) };
}

pub fn set_language(language: Language) {
    LANGUAGE.set(language);
}

pub fn current_language() -> Language {
    LANGUAGE.get()
}

pub fn localized(text: impl Into<String>) -> String {
    translate(&text.into())
}

pub fn select_label(label: &str) -> String {
    if current_language().is_english() {
        format!("Select {}", translate(label))
    } else {
        format!("选择{label}")
    }
}

pub fn translate(text: &str) -> String {
    if !current_language().is_english() {
        return text.to_owned();
    }
    if let Some(translation) = exact_translation(text) {
        return translation.to_owned();
    }
    dynamic_translation(text)
}

pub struct RichText;

impl RichText {
    #[allow(clippy::new_ret_no_self)] // Namespace wrapper preserves the existing `RichText::new` call sites.
    pub fn new(text: impl Into<String>) -> egui::RichText {
        egui::RichText::new(localized(text))
    }
}

pub fn localized_button(text: impl Into<WidgetText>) -> egui::Button<'static> {
    egui::Button::new(localize_widget_text(text))
}

pub trait LocalizedUi {
    fn localized_label(&mut self, text: impl Into<WidgetText>) -> egui::Response;
    fn localized_button(&mut self, text: impl Into<String>) -> egui::Response;
    fn localized_checkbox(
        &mut self,
        checked: &mut bool,
        text: impl Into<WidgetText>,
    ) -> egui::Response;
}

impl LocalizedUi for egui::Ui {
    fn localized_label(&mut self, text: impl Into<WidgetText>) -> egui::Response {
        self.label(localize_widget_text(text))
    }

    fn localized_button(&mut self, text: impl Into<String>) -> egui::Response {
        self.button(localized(text))
    }

    fn localized_checkbox(
        &mut self,
        checked: &mut bool,
        text: impl Into<WidgetText>,
    ) -> egui::Response {
        self.checkbox(checked, localize_widget_text(text))
    }
}

pub trait LocalizedResponse {
    fn localized_hover(self, text: impl Into<String>) -> Self;
}

impl LocalizedResponse for egui::Response {
    fn localized_hover(self, text: impl Into<String>) -> Self {
        self.on_hover_text(localized(text))
    }
}

pub fn localize_widget_text(text: impl Into<WidgetText>) -> WidgetText {
    match text.into() {
        WidgetText::Text(text) => WidgetText::Text(translate(&text)),
        text => text,
    }
}

fn exact_translation(text: &str) -> Option<&'static str> {
    Some(match text {
        "跟随系统" => "Follow system",
        "浅色" => "Light",
        "深色" => "Dark",
        "外观：跟随系统" => "Appearance: Follow system",
        "外观：浅色" => "Appearance: Light",
        "外观：深色" => "Appearance: Dark",
        "浏览器" => "Browser",
        "当前浏览器" => "Current browser",
        "浏览器程序" => "Browser program",
        "渠道" => "Channel",
        "构建" => "Build",
        "来源" => "Source",
        "当前版本" => "Current version",
        "上游版本" => "Upstream version",
        "检查更新" => "Check for updates",
        "下载并安装" => "Download and install",
        "下载并升级" => "Download and upgrade",
        "检查并安装/升级" => "Check and install/upgrade",
        "下载上游版本" => "Download upstream",
        "重新下载并替换" => "Download again and replace",
        "选择本地安装包" => "Choose local installer package",
        "更换本地安装包" => "Change local installer package",
        "清除本地安装包" => "Clear local installer package",
        "使用本地包重新安装" => "Reinstall from local package",
        "使用本地包升级" => "Upgrade from local package",
        "使用本地包安装" => "Install from local package",
        "使用本地包安装/升级" => "Install/upgrade from local package",
        "选择当前浏览器、渠道和构建对应的官方安装包；安装时仍会校验版本、大小和摘要，并可能需要联网准备便携运行时" => {
            "Choose the official installer for the current browser, channel, and build. Installation still verifies version, size, and digest, and may require network access for portable runtimes."
        }
        "渠道不同" => "Channel mismatch",
        "使用 Beta 渠道安装" => "Install using Beta channel",
        "使用 ESR 渠道安装" => "Install using ESR channel",
        "使用 Release 渠道安装" => "Install using Release channel",
        "使用 Stable 渠道安装" => "Install using Stable channel",
        "使用本地 Beta 离线包安装" => "Install from local Beta package",
        "使用本地 ESR 离线包安装" => "Install from local ESR package",
        "使用本地 Release 离线包安装" => "Install from local Release package",
        "使用本地 Stable 离线包安装" => "Install from local Stable package",
        "路径与用户数据" => "Paths and user data",
        "检查配置" => "Check configuration",
        "快捷方式：开" => "Shortcut: On",
        "快捷方式：关" => "Shortcut: Off",
        "浏览器根目录" => "Browser root",
        "程序目录" => "Program directory",
        "数据目录" => "Data directory",
        "用户数据目录" => "User data directory",
        "Profile 目录" => "Profile directory",
        "Profile 位置" => "Profile location",
        "缓存目录" => "Cache directory",
        "书签、历史、Cookie、登录状态和浏览器设置所在的总目录" => {
            "The root containing bookmarks, history, cookies, sign-in state, and browser settings"
        }
        "该浏览器发行包使用独立 Profile 位置；通常无需修改" => {
            "This browser distribution needs a separate Profile location; usually no change is needed"
        }
        "高级：具体 Profile 子目录；通常无需修改，留空会按浏览器布局自动恢复" => {
            "Advanced: the specific Profile subdirectory. Usually no change is needed; leaving it empty restores the browser-layout default"
        }
        "缓存总目录；留空时跟随 Profile。已安装的浏览器修改路径后，请在“检查配置”中修正。" => {
            "Cache root; leave empty to follow Profile. For an installed browser, use Check configuration after changing this path."
        }
        "缓存总目录；浏览器缓存会在此目录下按 Profile 存放。已安装的浏览器修改路径后，请在“检查配置”中修正。" => {
            "Cache root. Browser cache is stored here by Profile. For an installed browser, use Check configuration after changing this path."
        }
        "Chrome++ 文件位于浏览器程序目录" => {
            "Chrome++ files are stored in the browser program directory"
        }
        "浏览器安装或升级时会自动下载、校验并放置 Chrome++，无需手工下载" => {
            "Installing or upgrading the browser automatically downloads, verifies, and installs Chrome++; no manual download is needed"
        }
        "例如 .\\Chrome\\App" => "Example: .\\Chrome\\App",
        "可单独覆盖" => "Optional override",
        "留空时由便携运行时决定" => "Leave empty for the portable runtime default",
        "留空跟随 Profile；自定义会同时移动临时文件" => {
            "Leave empty to follow Profile; a custom path also moves temporary files"
        }
        "留空跟随 Profile（推荐）" => "Leave empty to follow Profile (recommended)",
        "跟随 PortableData" => "Follow PortableData",
        "跟随 Profile" => "Follow Profile",
        "留空跟随 Profile" => "Leave empty to follow Profile",
        "安装后自动在浏览器根目录创建快捷方式；点击关闭" => {
            "Create a shortcut in the Browser Root after installation; click to disable"
        }
        "已关闭自动创建；点击开启" => {
            "Automatic shortcut creation is off; click to enable"
        }
        "下载加速" => "Download acceleration",
        "启用内置下载加速站" => "Enable built-in accelerators",
        "添加额外的自定义加速站（可选）" => "Add custom accelerators (optional)",
        "加速站前缀" => "Accelerator prefix",
        "加速站 URL 前缀；程序会在原始下载地址前拼接此文本" => {
            "Accelerator URL prefix; prepended to the original download URL"
        }
        "添加" => "Add",
        "删除" => "Delete",
        "选择" => "Select",
        "下移" => "Move down",
        "上移" => "Move up",
        "代理与超时" => "Proxy and timeout",
        "代理地址" => "Proxy address",
        "留空时使用系统代理" => "Leave empty to use the system proxy",
        "超时" => "Timeout",
        "代理请求超时（秒）" => "Proxy request timeout (seconds)",
        "秒" => "seconds",
        "个文件" => "files",
        "下载缓存" => "Download cache",
        "清理下载缓存" => "Clear download cache",
        "配置位置" => "Configuration location",
        "系统用户目录（默认）" => "User configuration directory (default)",
        "程序目录（便携）" => "Program directory (portable)",
        "当前配置文件" => "Current config file",
        "保存在 %LOCALAPPDATA%\\browser-harbor\\BrowserHarbor.json" => {
            "Stored at %LOCALAPPDATA%\\browser-harbor\\BrowserHarbor.json"
        }
        "将 BrowserHarbor.json 保存在 Browser Harbor EXE 旁，可随程序一起带走" => {
            "Keep BrowserHarbor.json beside the Browser Harbor EXE so it can travel with the program"
        }
        "便携模式要求 EXE 所在目录可写（避免放在 C 盘根目录或 Program Files 等受保护位置）；浏览器绝对路径在新电脑上仍需重新定位。" => {
            "Portable mode requires a writable EXE directory (avoid protected paths like C:\\ or Program Files). Absolute browser paths still need relocation on a new PC."
        }
        "切换配置位置失败：当前目录受系统权限保护（如 C 盘根目录或 Program Files）。请将程序移至普通用户目录（如 D 盘或 C:\\Tools），或右键以管理员身份运行。" => {
            "Failed to switch configuration location: The current directory is protected by Windows permissions (such as C:\\ or Program Files). Move Browser Harbor to a standard user directory (such as D:\\ or C:\\Tools), or run as administrator."
        }
        "自动保存失败：当前目录受系统权限保护。请将程序移至普通用户目录，或以管理员身份运行。" => {
            "Autosave failed: The current directory is protected by Windows permissions. Move Browser Harbor to a standard user directory, or run as administrator."
        }
        "配置位置已切换" => "Configuration location changed",
        "切换配置位置失败" => "Could not change configuration location",
        "Chrome++ 集成" => "Chrome++ integration",
        "配置文件" => "Configuration file",
        "模式" => "Mode",
        "状态" => "Status",
        "Chrome++ 源码与 GPL-3.0 许可证" => "Chrome++ source and GPL-3.0 license",
        "版本" => "Version",
        "已安装" => "Installed",
        "尚未安装" => "Not installed",
        "Chrome++ 尚未安装" => "Chrome++ is not installed",
        "尚未检测" => "Not detected",
        "目录不存在" => "Directory missing",
        "安装不完整" => "Installation incomplete",
        "版本未知" => "Version unknown",
        "未检测到本地浏览器版本" => "Local browser version was not detected",
        "重新下载并安装" => "Download and reinstall",
        "更新 Chrome++" => "Update Chrome++",
        "取消" => "Cancel",
        "将只替换 Chrome++ 文件，保留浏览器程序、用户数据和缓存；旧 INI 会先备份。" => {
            "Only Chrome++ files will be replaced. The browser program, user data and cache are preserved; the previous INI is backed up first."
        }
        "请保持所有当前浏览器实例关闭。点击更新后会再次检测；仍在运行的实例不会被强制结束。" => {
            "Keep every current browser instance closed. Running instances are checked again after confirmation and are never terminated."
        }
        "已取消 Chrome++ 更新" => "Chrome++ update cancelled",
        "同时更新 Chrome++" => "Update Chrome++ too",
        "继续浏览器操作" => "Continue browser operation",
        "已取消浏览器操作" => "Browser operation cancelled",
        "浏览器版本相同，继续会重新下载并替换浏览器，同时更新 Chrome++。" => {
            "The browser versions match. Continuing will download and replace the browser again and update Chrome++ at the same time."
        }
        "浏览器升级会替换程序目录，因此本次操作也会安装已检测到的 Chrome++ 新版本。" => {
            "Browser upgrade replaces the program directory, so this operation will also install the detected Chrome++ version."
        }
        "用户数据、缓存和可恢复的 Chrome++ 配置会保留；请先关闭当前浏览器的所有实例。" => {
            "User data, cache and recoverable Chrome++ settings are preserved. Close every instance of the current browser first."
        }
        "检查 Bush2021/chrome_plus 的最新正式版本" => {
            "Check the latest stable Bush2021/chrome_plus release"
        }
        "最新版本：{latest}；点击重新检查" => {
            "Latest version: {latest}; click to check again"
        }
        "从 INI 载入" => "Load from INI",
        "打开配置文件" => "Open configuration",
        "更多操作…" => "More actions…",
        "打开所在文件夹" => "Open containing folder",
        "备份当前 INI" => "Back up current INI",
        "恢复默认配置" => "Restore defaults",
        "启动与快捷键" => "Startup and shortcuts",
        "附加参数" => "Additional arguments",
        "例如 --disable-features=OutdatedBuildDetector" => {
            "Example: --disable-features=OutdatedBuildDetector"
        }
        "启动命令" => "Startup command",
        "浏览器启动后执行；不需要请留空" => {
            "Runs after the browser starts; leave empty if unnecessary"
        }
        "退出命令" => "Exit command",
        "多条命令用 ; 分隔，例如 " => "Separate multiple commands with ;, for example ",
        "多条命令用 ; 分隔，例如 \"D:\\App\\clean.bat\";" => {
            "Separate multiple commands with ;, for example \"D:\\App\\clean.bat\";"
        }
        "老板键" => "Boss key",
        "例如 Ctrl+Alt+B" => "Example: Ctrl+Alt+B",
        "翻译键" => "Translation key",
        "例如 Ctrl+Alt+T" => "Example: Ctrl+Alt+T",
        "启动和退出命令会由 Chrome++ 执行，请只填写可信命令。" => {
            "Chrome++ runs startup and exit commands; use trusted commands only."
        }
        "标签页与书签" => "Tabs and bookmarks",
        "双击关闭标签" => "Double-click closes tab",
        "右键关闭标签" => "Right-click closes tab",
        "保留最后一个标签" => "Keep last tab",
        "标签栏滚轮切换" => "Wheel switches tabs",
        "按住右键滚轮切换" => "Right-drag wheel switches tabs",
        "地址栏新标签打开" => "Open address-bar URLs in new tabs",
        "书签在新标签打开" => "Open bookmarks in new tabs",
        "禁用指定新标签页" => "Disable selected new tabs",
        "悬停自动切换标签" => "Switch tab on hover",
        "新标签匹配" => "New-tab matches",
        "悬停延迟" => "Hover delay",
        "兼容与策略" => "Compatibility and policies",
        "免系统验证查看密码" => "Show passwords without system verification",
        "启用 win32k" => "Enable win32k",
        "忽略企业策略" => "Ignore enterprise policies",
        "抑制错误升级提示" => "Suppress false upgrade notices",
        "按键映射" => "Key mapping",
        "每行一个映射，例如 F2=Ctrl+PageUp" => {
            "One mapping per line, for example F2=Ctrl+PageUp"
        }
        "移除全部按键映射" => "Remove all key mappings",
        "载入现有映射" => "Load current mappings",
        "支持 SourceKey=TargetKey 或 SourceKey=command:ID。" => {
            "Supports SourceKey=TargetKey or SourceKey=command:ID."
        }
        "未编辑时保留 INI 中已有的按键映射。" => {
            "Existing INI key mappings are kept until edited."
        }
        "程序目录未配置" => "Program directory is not configured",
        "检查结果：正常" => "Result: Healthy",
        "请确认重新定位后的路径" => "Confirm the relocated paths",
        "无法确认该程序属于当前浏览器；应用前请确认目录选择正确。" => {
            "Cannot confirm that this program belongs to the current browser; verify the selected directory before applying."
        }
        "请先关闭浏览器；其他便携参数和注释会保留。" => {
            "Close the browser first; other portable settings and comments are preserved."
        }
        "请先关闭浏览器；其他 Chrome++ 参数和注释会保留。" => {
            "Close the browser first; other Chrome++ settings and comments are preserved."
        }
        "修复 Chrome++ 路径" => "Repair Chrome++ paths",
        "修复便携配置" => "Repair portable configuration",
        "取消修复" => "Cancel repair",
        "确认修复" => "Confirm repair",
        "将备份 chrome++.ini 并只修复路径键。" => {
            "Back up chrome++.ini and repair only path keys."
        }
        "将备份 portable.ini 并只修复路径键。" => {
            "Back up portable.ini and repair only path keys."
        }
        "将备份 tmemutil.ini 并只修复路径键。" => {
            "Back up tmemutil.ini and repair only path keys."
        }
        "将应用以下路径" => "The following paths will be applied",
        "取消重新定位" => "Cancel relocation",
        "应用" => "Apply",
        "关闭" => "Close",
        "重新定位…" => "Relocate…",
        "选择移动后的浏览器根目录" => "Choose relocated browser root",
        "版本与上游相同" => "Same version as upstream",
        "继续将重新下载并替换浏览器程序目录。用户数据、缓存和可按规则恢复的 Chrome++ 配置会保留。" => {
            "Continuing will download and replace the browser program. User data, cache, and recoverable Chrome++ settings are preserved."
        }
        "继续替换" => "Continue replacement",
        "保留当前版本" => "Keep current version",
        "浏览器身份不一致" => "Browser identity mismatch",
        "渠道与现有安装不一致" => "Installed channel differs",
        "继续安装或替换可能覆盖另一个浏览器的程序文件。建议重新选择目录；只有确认这是自定义构建时才继续。" => {
            "Installing or replacing may overwrite another browser's program files. Choose another directory unless this is an intentional custom build."
        }
        "继续会替换当前程序目录，但保留用户数据和缓存。若要并行保留两个渠道，请先修改 Browser Root。" => {
            "Continuing replaces the current program directory while preserving user data and cache. To keep both channels, change Browser Root first."
        }
        "仍然继续" => "Continue anyway",
        "仍然替换" => "Replace anyway",
        "返回修改" => "Go back",
        "重新选择目录" => "Choose another directory",
        "现有安装渠道与当前选择不一致，等待确认" => {
            "The installed channel differs from the current selection; waiting for confirmation"
        }
        "已取消渠道切换" => "Channel change cancelled",
        "自动保存已开启" => "Autosave on",
        "界面语言" => "Interface language",
        "切换界面语言" => "Switch interface language",
        "查看发布版本" => "View releases",
        "全局设置" => "Global settings",
        "再次点击返回浏览器设置" => "Click again to return to browser settings",
        "全局设置：下载加速、代理、超时与缓存清理" => {
            "Global settings: accelerators, proxy, timeout, and cache cleanup"
        }
        "取消任务" => "Cancel task",
        "未安装" => "Not installed",
        "正常" => "Healthy",
        "有警告" => "Warning",
        "有错误" => "Error",
        "程序" => "Program",
        "便携运行时" => "Portable runtime",
        "便携配置" => "Portable configuration",
        "Chromium 系" => "Chromium",
        "Firefox 系" => "Firefox",
        "程序版本资源" => "Executable version resource",
        "程序文件版本" => "Executable file version",
        "Brave 产品版本" => "Brave product version",
        "上次安装记录" => "Install receipt",
        "完整集成" => "Full integration",
        "实验性 DataOnly" => "Experimental DataOnly",
        "不使用" => "Disabled",
        "已最新" => "Up to date",
        "可更新" => "Update available",
        "Mozilla 官方" => "Mozilla official",
        "GitHub Release" => "GitHub Release",
        "LibreWolf 官方" => "LibreWolf official",
        "Catsxp 官方" => "Catsxp official",
        "SkyKakapo 官方" => "SkyKakapo official",
        "Turbo 官方" => "Turbo official",
        "无法比较" => "Cannot compare",
        "尚未检查上游版本" => "Upstream not checked",
        "与上游版本相同" => "Same as upstream",
        "上游有新版本" => "Upstream has a newer version",
        "上游版本较旧" => "Upstream is older",
        "上游版本不同" => "Different upstream version",
        "版本无法可靠比较" => "Versions cannot be compared reliably",
        "未检测到" => "Not detected",
        "未检测" => "Not detected",
        "配置加载失败" => "Configuration load failed",
        "请先设置浏览器根目录" => "Set the Browser Root first",
        "浏览器身份不一致，等待确认" => {
            "Browser identity mismatch; waiting for confirmation"
        }
        "已取消浏览器安装" => "Browser installation cancelled",
        "已取消同版本替换" => "Same-version replacement cancelled",
        "缓存结果；" => "Cached result; ",
        "缓存上游版本" => "Cached upstream version",
        "签名发布清单" => "Signed release manifest",
        "签名发布清单；" => "Signed release manifest; ",
        "官方元数据不可用，使用签名发布清单" => {
            "Official metadata unavailable; using the signed release manifest"
        }
        "签名发布清单已过期，但使用它完成本次安装" => {
            "The signed release manifest has expired; using it for this installation"
        }
        "官方元数据不可用；签名发布清单已过期，但使用它完成本次安装" => {
            "Official metadata is unavailable and the signed release manifest has expired; using it for this installation"
        }
        "刚刚" => "just now",
        "不足 1 分钟前" => "less than a minute ago",
        "完整模式：以下选项对应 Chrome++ 的全部公开配置。" => {
            "Full mode: these options cover all public Chrome++ settings."
        }
        "实验性 DataOnly 只写入用户数据与缓存路径，不开放标签或策略补丁。" => {
            "Experimental DataOnly writes only data and cache paths; tab and policy patches are disabled."
        }
        "该浏览器的 Chrome++ 兼容性仍为实验性；完整功能已开放，若启动异常请关闭相关选项。" => {
            "Chrome++ compatibility is experimental for this browser. Full settings are available; disable affected options if startup fails."
        }
        "先备份当前 INI，再将可编辑配置恢复为默认值" => {
            "Back up the current INI, then restore editable settings to defaults"
        }
        "win32k 仅在 Chrome++ 导致启动崩溃时启用；免验证查看密码会降低本机账户保护。" => {
            "Enable win32k only if Chrome++ causes startup crashes. Showing passwords without verification weakens local account protection."
        }
        "官方发布，用户确认后安装" => {
            "Official release; install after user confirmation"
        }
        "BETA，用户确认后安装" => "Beta release; install after user confirmation",
        "AVX512 构建需要 CPU 支持 AVX512" => "AVX512 builds require CPU AVX512 support",
        "选择下载包对应的处理器构建" => "Choose the CPU build matching the package",
        "未添加自定义加速站；仍会尝试内置 GitHub 加速入口。" => {
            "No custom accelerators added; built-in GitHub accelerators will still be tried."
        }
        "加速站只改变 GitHub 传输路径，不改变版本、长度或摘要校验。" => {
            "Accelerators change only the GitHub transport path, never version, size, or digest verification."
        }
        "最多添加 20 个加速站" => "At most 20 accelerators are allowed",
        "加速站已经存在" => "This accelerator already exists",
        "仅删除可重新下载的安装包，不影响浏览器数据。" => {
            "Cached installers only; browser data stays."
        }
        "无法读取缓存目录" => "Cannot read the cache directory",
        "以下网络设置应用于全部浏览器，不属于当前选中的单个浏览器。" => {
            "These network settings apply to Browser Harbor, not to one selected browser."
        }
        "正在准备任务…" => "Preparing…",
        "正在检查官方发布…" => "Checking official release…",
        "正在校验下载文件…" => "Verifying download…",
        "正在解压…" => "Extracting…",
        "正在替换程序目录…" => "Replacing program files…",
        "任务已取消" => "Task cancelled",
        "Chrome++ 配置已自动写入" => "Chrome++ configuration saved automatically",
        _ => return None,
    })
}

fn dynamic_translation(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("Chrome++ 上游版本已从 ")
        && let Some((previous, current)) = rest.split_once(" 变化为 ")
        && let Some(current) = current.strip_suffix("，请重新检查")
    {
        return format!(
            "Chrome++ upstream changed from {previous} to {current}; check again before updating"
        );
    }
    if let Some(rest) = text.strip_prefix("本地版本 ")
        && let Some((current, upstream)) = rest.split_once(" 与上游 ")
        && let Some(upstream) = upstream.strip_suffix(" 相同，等待确认")
    {
        return format!(
            "Local version {current} matches upstream {upstream}; waiting for confirmation"
        );
    }
    if let Some(rest) = text.strip_prefix("缓存结果；检查时间：") {
        return format!("Cached result; checked {rest}");
    }
    if let Some(rest) = text.strip_prefix("目录中的程序是 ")
        && let Some((program, rest)) = rest.split_once("，当前目标是 ")
        && let Some(target) = rest.strip_suffix("；安装前会再次确认")
    {
        return format!(
            "Directory contains {program}, but the current target is {target}; confirmation is required before installation"
        );
    }
    if let Some(rest) = text.strip_prefix("目录中的程序：")
        && let Some((program, product)) = rest.split_once("（")
        && let Some(product) = product.strip_suffix("）")
    {
        return format!("Program in directory: {program} ({product})");
    }
    for (prefix, replacement) in [
        ("当前浏览器：", "Current browser: "),
        ("浏览器：", "Browser: "),
        ("当前目标：", "Current target: "),
        ("当前安装：", "Currently installed: "),
        ("程序标识：", "Program identity: "),
        ("本次选择：", "Selected for install: "),
        ("目录中的程序：", "Program in directory: "),
        ("目录中的程序是 ", "Directory contains "),
        ("版本来源：", "Version source: "),
        ("目录不存在：", "Directory missing: "),
        ("未找到浏览器程序：", "Browser executable not found: "),
        ("上次安装记录：", "Last install receipt: "),
        ("检查结果：", "Result: "),
        ("外观：", "Appearance: "),
        ("更新来源：", "Update source: "),
        ("选择", "Select "),
        ("已安装 ", "Installed "),
        ("检查完成：", "Check complete: "),
        ("正在下载 ", "Downloading "),
        ("最新版本：", "Latest version: "),
        ("检查时间：", "Checked "),
        ("当前本地安装包：", "Current local installer package: "),
        ("快捷方式已创建：", "Shortcut created: "),
        ("已重新定位到：", "Relocated to: "),
        ("已清理下载缓存：", "Download cache cleared: "),
        ("INI 已备份：", "INI backed up: "),
        (
            "便携配置已修复；原配置备份到 ",
            "Portable configuration repaired; backup: ",
        ),
        (
            "Chrome++ 路径已修复；原配置备份到 ",
            "Chrome++ paths repaired; backup: ",
        ),
        ("本地版本 ", "Local version "),
        ("Chrome++ 已是最新版本：", "Chrome++ is up to date: "),
        ("Chrome++ 发现上游版本：", "Chrome++ upstream version: "),
        ("Chrome++ 最新版本：", "Latest Chrome++: "),
        ("Chrome++ 检查完成：", "Chrome++ check complete: "),
        ("Chrome++ 已更新到 ", "Chrome++ updated to "),
        (
            "浏览器仍在运行，请手工关闭所有实例后再更新 Chrome++：",
            "The browser is still running. Close every instance manually before updating Chrome++: ",
        ),
        (
            "无法检查浏览器运行状态：",
            "Cannot inspect browser process state: ",
        ),
        (
            "官方元数据缺少可信摘要，签名发布清单也不可用：",
            "Official metadata has no trusted digest and the signed manifest is unavailable: ",
        ),
        ("官方元数据不可用：", "Official metadata unavailable: "),
        ("签名发布清单没有 ", "Signed release manifest has no "),
        ("无法检查配置：", "Cannot check configuration: "),
        ("无法修复 Chrome++ 路径：", "Cannot repair Chrome++ paths: "),
        (
            "无法修复便携配置：",
            "Cannot repair portable configuration: ",
        ),
        ("无法重新定位：", "Cannot relocate: "),
        ("自动保存失败：", "Autosave failed: "),
        ("配置加载失败：", "Configuration load failed: "),
        ("保存失败：", "Save failed: "),
        (
            "读取 Chrome++ 配置失败：",
            "Cannot read Chrome++ configuration: ",
        ),
        ("读取按键映射失败：", "Cannot read key mappings: "),
        ("无法打开配置文件：", "Cannot open configuration file: "),
        (
            "无法打开配置文件所在目录：",
            "Cannot open configuration folder: ",
        ),
        ("清理下载缓存失败：", "Cannot clear download cache: "),
        ("恢复默认配置失败：", "Cannot restore defaults: "),
        ("备份 INI 失败：", "Cannot back up INI: "),
        (
            "自动写入 Chrome++ 配置失败：",
            "Cannot save Chrome++ configuration: ",
        ),
        ("创建快捷方式失败：", "Cannot create shortcut: "),
        (
            "安装完成，但保存状态失败：",
            "Installed, but saving state failed: ",
        ),
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return format!("{replacement}{rest}");
        }
    }
    if let Some(rest) = text.strip_suffix("（上次安装）") {
        return format!("{rest} (last install)");
    }
    for (suffix, replacement) in [
        (" 分钟前", " minutes ago"),
        (" 小时前", " hours ago"),
        (" 天前", " days ago"),
    ] {
        if let Some(rest) = text.strip_suffix(suffix) {
            return format!("{rest}{replacement}");
        }
    }
    if let Some(rest) = text.strip_prefix("将备份 ") {
        return format!("Back up {rest}");
    }
    if let Some(rest) = text.strip_prefix("例如 ") {
        return format!("Example: {}", rest.replace(" 或 ", " or "));
    }
    if let Some(rest) = text.strip_prefix("已恢复默认配置；原文件备份到 ") {
        return format!("Defaults restored; original backed up to {rest}");
    }
    if text.contains("本地版本与上游版本相同") {
        return text
            .replace(
                "本地版本与上游版本相同",
                "Local and upstream versions match",
            )
            .replace("等待确认替换", "waiting for replacement confirmation");
    }
    if text.starts_with("当前目录已安装 ") && text.contains(" 渠道（") {
        return text
            .replace(
                "当前目录已安装 ",
                "Installed version in directory belongs to ",
            )
            .replace(" 渠道（", " channel (")
            .replace("）；当前选择的是 ", "); currently selected is ")
            .replace(" 渠道（", " channel (")
            .replace('）', ")");
    }
    if text.starts_with("Libportable 暂不支持 ") {
        return text
            .replace("Libportable 暂不支持 ", "Libportable does not yet support ")
            .replace(" 版本 ", " version ")
            .replace(
                "，请等待适配或选择兼容版本。",
                "; wait for compatibility or select a supported version.",
            );
    }
    if let Some(rest) = text.strip_prefix("便携运行时准备失败：") {
        return format!("Portable runtime preparation failed: {rest}");
    }
    if text.starts_with("文件操作受系统权限保护（") {
        return text
            .replace("文件操作受系统权限保护（", "File operation was blocked by Windows permissions (")
            .replace("）。请将浏览器根目录移至普通用户目录（如 D 盘或 C:\\Tools），或右键以管理员身份运行。", "). Move the browser root to a standard user directory (such as D:\\ or C:\\Tools), or run as administrator.");
    }
    if text.contains(" 已安装") {
        return text
            .replace(" 已安装", " installed")
            .replace("；快捷方式已创建：", "; shortcut created: ")
            .replace("；自动创建快捷方式失败：", "; automatic shortcut failed: ")
            .replace("；Chrome++ 已从", "; Chrome++ changed from")
            .replace("更新到", "to")
            .replace("，请重新检查配置", "; check configuration again");
    }
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::{localized, set_language, translate};
    use crate::domain::settings::Language;

    #[test]
    fn switches_fixed_and_dynamic_ui_text() {
        set_language(Language::Chinese);
        assert_eq!(translate("浏览器"), "浏览器");

        set_language(Language::English);
        assert_eq!(translate("浏览器"), "Browser");
        assert_eq!(translate("当前浏览器：Firefox"), "Current browser: Firefox");
        assert_eq!(translate("159.0（上次安装）"), "159.0 (last install)");
        assert_eq!(localized("检查更新"), "Check for updates");
        assert_eq!(localized("加速站前缀"), "Accelerator prefix");
        assert_eq!(localized("当前配置文件"), "Current config file");
        assert_eq!(
            localized("留空跟随 Profile"),
            "Leave empty to follow Profile"
        );
        assert_eq!(localized("检查配置"), "Check configuration");
        assert_eq!(
            localized(
                "自动保存失败：当前目录受系统权限保护。请将程序移至普通用户目录，或以管理员身份运行。"
            ),
            "Autosave failed: The current directory is protected by Windows permissions. Move Browser Harbor to a standard user directory, or run as administrator."
        );
        assert_eq!(
            localized(
                "文件操作受系统权限保护（C:\\Program Files\\Chrome）。请将浏览器根目录移至普通用户目录（如 D 盘或 C:\\Tools），或右键以管理员身份运行。"
            ),
            "File operation was blocked by Windows permissions (C:\\Program Files\\Chrome). Move the browser root to a standard user directory (such as D:\\ or C:\\Tools), or run as administrator."
        );
        assert_eq!(localized("渠道不同"), "Channel mismatch");
        assert_eq!(localized("使用 ESR 渠道安装"), "Install using ESR channel");
        assert_eq!(
            localized("使用本地 ESR 离线包安装"),
            "Install from local ESR package"
        );
        assert_eq!(
            localized("当前目录已安装 Release 渠道（159.0）；当前选择的是 ESR 渠道（115.44.0esr）"),
            "Installed version in directory belongs to Release channel (159.0); currently selected is ESR channel (115.44.0esr)"
        );
        assert_eq!(
            localized("Libportable 暂不支持 Zen 版本 9.99，请等待适配或选择兼容版本。"),
            "Libportable does not yet support Zen version 9.99; wait for compatibility or select a supported version."
        );
        assert_eq!(
            localized("便携运行时准备失败：组件损坏"),
            "Portable runtime preparation failed: 组件损坏"
        );
        assert_eq!(
            localized(
                "缓存总目录；浏览器缓存会在此目录下按 Profile 存放。已安装的浏览器修改路径后，请在“检查配置”中修正。"
            ),
            "Cache root. Browser cache is stored here by Profile. For an installed browser, use Check configuration after changing this path."
        );
        assert_eq!(
            localized(r"例如 .\Helium 或 D:\Tools\Browser\Helium"),
            r"Example: .\Helium or D:\Tools\Browser\Helium"
        );
        assert_eq!(
            localized("代理请求超时（秒）"),
            "Proxy request timeout (seconds)"
        );
        assert_eq!(
            localized("渠道与现有安装不一致"),
            "Installed channel differs"
        );
        assert_eq!(translate("签名发布清单"), "Signed release manifest");
        assert_eq!(
            translate("官方元数据不可用，使用签名发布清单"),
            "Official metadata unavailable; using the signed release manifest"
        );

        set_language(Language::Chinese);
    }
}
