//! Chrome++ configuration and runtime integration tab.

use super::*;
use eframe::egui::{Align, Color32, CornerRadius, Layout, Stroke};

impl BrowserHarborApp {
    pub(super) fn show_chrome_plus(&mut self, ui: &mut egui::Ui) {
        let palette = palette(ui);
        let target = self.state.selected_target();
        let mode = target.chrome_plus_mode();
        let mut config = self.state.draft().targets[&target].clone();
        let ini_path = chrome_plus_ini_path(target, &config, self.state.tool_root());
        if !self.chrome_plus_versions.contains_key(&target) {
            let detected = chrome_plus_program_directory(target, &config, self.state.tool_root())
                .and_then(|directory| {
                    crate::runtime::chromeplus::installed_version(&directory).ok()
                });
            self.chrome_plus_versions.insert(target, detected);
        }
        let detected_version = self.chrome_plus_versions.get(&target).cloned().flatten();
        let installed = ini_path.is_some();
        let installed_version = if installed {
            detected_version.or_else(|| config.chrome_plus_version.clone())
        } else {
            None
        };
        let stored_chrome_plus_version = config.chrome_plus_version.clone();
        if installed_version.is_some() {
            config.chrome_plus_version = installed_version.clone();
        }
        let backup_dir = config
            .browser_root
            .as_deref()
            .and_then(|root| resolve_preview_path(self.state.tool_root(), root))
            .map(|root| root.join("backups"));
        let latest_version = self.chrome_plus_latest.get(&target).cloned();
        let update_label = chrome_plus_update_button_label(
            installed_version.as_deref(),
            latest_version.as_deref(),
        );
        let mut changed = config.chrome_plus_version != stored_chrome_plus_version;
        let mut ini_changed = false;
        let mut pending_status = None;
        if self.state.has_pending_chromeplus()
            && ui.localized_button("重试应用 Chrome++ 配置").clicked()
            && self.auto_save()
        {
            self.status_message = Some("Chrome++ 配置已自动写入".to_owned());
        }
        if !self.state.has_pending_chromeplus()
            && !self.chrome_plus_ini_loaded.contains(&target)
            && let Some(path) = &ini_path
        {
            match crate::runtime::chromeplus::read_config_from_ini(path, &config.chrome_plus) {
                Ok(loaded) => {
                    changed |= loaded != config.chrome_plus;
                    config.chrome_plus = loaded;
                    self.chrome_plus_ini_loaded.insert(target);
                }
                Err(error) => {
                    pending_status = Some(format!("读取 Chrome++ 配置失败：{error}"));
                }
            }
        }
        let mut open_ini = false;
        let mut open_ini_folder = false;
        let mut backup_ini = false;
        let mut restore_defaults = false;
        let mut check_update = false;
        let mut request_update = false;
        let mut load_keymapping = false;
        let mut load_ini = false;

        ui.columns(2, |columns| {
            section_with_header(
                &mut columns[0],
                "Chrome++ 集成",
                |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(86.0, 24.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            let update_available = update_label == "可更新";
                            let update_hover = if update_available {
                                chrome_plus_update_hover(
                                    self.state.draft().language,
                                    installed_version.as_deref().unwrap_or("—"),
                                    latest_version.as_deref().unwrap_or("—"),
                                )
                            } else {
                                latest_version.as_deref().map_or_else(
                                    || "检查 Bush2021/chrome_plus 的最新正式版本".to_owned(),
                                    |latest| format!("最新版本：{latest}；点击重新检查"),
                                )
                            };
                            let update = ui
                                .add_enabled(
                                    self.operation.is_none(),
                                    localized_button("")
                                        .min_size(egui::vec2(24.0, 24.0))
                                        .frame(false),
                                )
                                .localized_hover(update_hover);
                            let hovered = update.hovered() || update.has_focus();
                            ui.painter().rect_filled(
                                update.rect,
                                CornerRadius::same(5),
                                if hovered {
                                    palette.hover
                                } else {
                                    Color32::TRANSPARENT
                                },
                            );
                            if update.has_focus() {
                                ui.painter().rect_stroke(
                                    update.rect,
                                    CornerRadius::same(5),
                                    Stroke::new(1.0, palette.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let update_color = match update_label.as_str() {
                                "已最新" => palette.success,
                                "可更新" => palette.accent_text,
                                _ => palette.text,
                            };
                            let icon_color = if hovered {
                                palette.accent_text
                            } else {
                                update_color
                            };
                            if update_available {
                                paint_update_icon(ui.painter(), update.rect.center(), icon_color);
                            } else {
                                paint_refresh_icon(ui.painter(), update.rect.center(), icon_color);
                            }
                            let accessible_label = if update_available {
                                chrome_plus_update_action_label(
                                    self.state.draft().language,
                                    latest_version.as_deref().unwrap_or("—"),
                                )
                            } else {
                                localized(update_label.as_str())
                            };
                            update.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::Button,
                                    self.operation.is_none(),
                                    accessible_label.clone(),
                                )
                            });
                            if update.clicked() {
                                if update_available {
                                    request_update = true;
                                } else {
                                    check_update = true;
                                }
                            }

                            ui.add_space(3.0);
                            let version_text = installed_version
                                .as_deref()
                                .map_or_else(|| "—".to_owned(), |version| format!("v{version}"));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(version_text).monospace().strong().color(
                                        if installed_version.is_some() {
                                            palette.text
                                        } else {
                                            palette.muted
                                        },
                                    ),
                                )
                                .show_tooltip_when_elided(false)
                                .truncate(),
                            )
                            .localized_hover(if installed {
                                "已安装的 Chrome++ 版本"
                            } else {
                                "Chrome++ 尚未安装"
                            });
                        },
                    );
                },
                |ui| {
                    ui.horizontal(|ui| {
                        metadata_label(ui, "模式", 72.0);
                        ui.localized_label(RichText::new(chrome_plus_label(mode)).strong().color(
                            if mode == ChromePlusMode::DataOnly {
                                palette.warning
                            } else {
                                palette.text
                            },
                        ));
                    });
                    ui.horizontal(|ui| {
                        metadata_label(ui, "状态", 72.0);
                        ui.localized_label(if installed {
                            "已安装"
                        } else {
                            "尚未安装"
                        })
                        .localized_hover(if installed {
                            "Chrome++ 文件位于浏览器程序目录"
                        } else {
                            "浏览器安装或升级时会自动下载、校验并放置 Chrome++，无需手工下载"
                        });
                    });
                    ui.horizontal(|ui| {
                        metadata_label(ui, "来源", 72.0);
                        ui.hyperlink_to(
                            localized("Chrome++ 源码与 GPL-3.0 许可证"),
                            CHROME_PLUS_SOURCE_URL,
                        )
                        .on_hover_text(CHROME_PLUS_SOURCE_URL);
                    });
                },
            );
            section_with_header(
                &mut columns[1],
                "配置文件",
                |_| {},
                |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let button_width = ui.available_width();
                    load_ini = ui
                        .add_enabled(
                            ini_path.is_some(),
                            localized_button("从 INI 载入")
                                .min_size(egui::vec2(button_width, 24.0)),
                        )
                        .clicked();
                    open_ini = ui
                        .add_enabled(
                            ini_path.is_some(),
                            localized_button("打开配置文件")
                                .min_size(egui::vec2(button_width, 24.0)),
                        )
                        .clicked();
                    let menu = egui::containers::menu::MenuButton::from_button(
                        localized_button("更多操作…").min_size(egui::vec2(button_width, 24.0)),
                    );
                    menu.ui(ui, |ui| {
                        if ui
                            .add_enabled(ini_path.is_some(), localized_button("打开所在文件夹"))
                            .clicked()
                        {
                            open_ini_folder = true;
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                ini_path.is_some() && backup_dir.is_some(),
                                localized_button("备份当前 INI"),
                            )
                            .clicked()
                        {
                            backup_ini = true;
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                ini_path.is_some() && backup_dir.is_some(),
                                localized_button("恢复默认配置"),
                            )
                            .localized_hover("先备份当前 INI，再将可编辑配置恢复为默认值")
                            .clicked()
                        {
                            restore_defaults = true;
                            ui.close();
                        }
                    });
                },
            );
        });
        if target.support_level() == SupportLevel::Experimental {
            ui.add_space(5.0);
            ui.localized_label(
                RichText::new(
                    "该浏览器的 Chrome++ 兼容性仍为实验性；完整功能已开放，若启动异常请关闭相关选项。",
                )
                .size(11.0)
                .color(palette.warning),
            );
        }
        ui.add_space(4.0);
        if mode == ChromePlusMode::Full {
            ui.localized_label(
                RichText::new("完整模式：以下选项对应 Chrome++ 的全部公开配置。")
                    .size(11.0)
                    .color(palette.muted),
            );
        } else if mode == ChromePlusMode::DataOnly {
            ui.localized_label(
                RichText::new("实验性 DataOnly 只写入用户数据与缓存路径，不开放标签或策略补丁。")
                    .size(11.5)
                    .color(palette.muted),
            );
        }

        if mode == ChromePlusMode::Full {
            ui.add_space(7.0);
            let section_changed = section(ui, "启动与快捷键", |ui| {
                let mut section_changed = text_field(
                    ui,
                    "附加参数",
                    &mut config.chrome_plus.command_line,
                    "例如 --disable-features=OutdatedBuildDetector",
                )
                .changed();
                section_changed |= text_field(
                    ui,
                    "启动命令",
                    &mut config.chrome_plus.launch_on_startup,
                    "浏览器启动后执行；不需要请留空",
                )
                .changed();
                section_changed |= text_field(
                    ui,
                    "退出命令",
                    &mut config.chrome_plus.launch_on_exit,
                    r#"多条命令用 ; 分隔，例如 "D:\App\clean.bat";"#,
                )
                .changed();
                ui.columns(2, |columns| {
                    section_changed |= text_field(
                        &mut columns[0],
                        "老板键",
                        &mut config.chrome_plus.boss_key,
                        "例如 Ctrl+Alt+B",
                    )
                    .changed();
                    section_changed |= text_field(
                        &mut columns[1],
                        "翻译键",
                        &mut config.chrome_plus.translate_key,
                        "例如 Ctrl+Alt+T",
                    )
                    .changed();
                });
                ui.localized_label(
                    RichText::new("启动和退出命令会由 Chrome++ 执行，请只填写可信命令。")
                        .size(10.5)
                        .color(palette.warning),
                );
                section_changed
            });
            changed |= section_changed;
            ini_changed |= section_changed;

            ui.add_space(7.0);
            let section_changed = section(ui, "标签页与书签", |ui| {
                let mut section_changed = false;
                ui.columns(2, |columns| {
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.double_click_close,
                            "双击关闭标签",
                        )
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.right_click_close,
                            "右键关闭标签",
                        )
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.keep_last_tab,
                            "保留最后一个标签",
                        )
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(&mut config.chrome_plus.wheel_tab, "标签栏滚轮切换")
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.wheel_tab_when_press_rbutton,
                            "按住右键滚轮切换",
                        )
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(
                            &mut config.chrome_plus.open_url_new_tab,
                            "地址栏新标签打开",
                        )
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(
                            &mut config.chrome_plus.open_bookmark_new_tab,
                            "书签在新标签打开",
                        )
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(
                            &mut config.chrome_plus.new_tab_disable,
                            "禁用指定新标签页",
                        )
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(&mut config.chrome_plus.hover_tab, "悬停自动切换标签")
                        .changed();
                });
                section_changed |= text_field(
                    ui,
                    "新标签匹配",
                    &mut config.chrome_plus.new_tab_disable_name,
                    "\"about:blank\",\"新建标签\"",
                )
                .changed();
                ui.horizontal(|ui| {
                    field_label(ui, "悬停延迟", FIELD_LABEL_WIDTH);
                    section_changed |= ui
                        .add_enabled(
                            config.chrome_plus.hover_tab,
                            egui::DragValue::new(&mut config.chrome_plus.hover_tab_delay)
                                .range(0..=10_000)
                                .suffix(" ms"),
                        )
                        .changed();
                });
                section_changed
            });
            changed |= section_changed;
            ini_changed |= section_changed;

            ui.add_space(7.0);
            let section_changed = section(ui, "兼容与策略", |ui| {
                let mut section_changed = false;
                ui.columns(2, |columns| {
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.show_password,
                            "免系统验证查看密码",
                        )
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(&mut config.chrome_plus.win32k, "启用 win32k")
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(&mut config.chrome_plus.ignore_policies, "忽略企业策略")
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(
                            &mut config.chrome_plus.suppress_false_upgrade_notification,
                            "抑制错误升级提示",
                        )
                        .changed();
                });
                ui.localized_label(
                    RichText::new(
                        "win32k 仅在 Chrome++ 导致启动崩溃时启用；免验证查看密码会降低本机账户保护。",
                    )
                    .size(10.5)
                    .color(palette.warning),
                );
                section_changed
            });
            changed |= section_changed;
            ini_changed |= section_changed;

            ui.add_space(7.0);
            let section_changed = section(ui, "按键映射", |ui| {
                let mut section_changed = false;
                if let Some(mapping) = &mut config.chrome_plus.keymapping {
                    section_changed |= ui
                        .add_sized(
                            [ui.available_width(), 68.0],
                            egui::TextEdit::multiline(mapping)
                                .hint_text(localized("每行一个映射，例如 F2=Ctrl+PageUp")),
                        )
                        .changed();
                    if ui.localized_button("移除全部按键映射").clicked() {
                        mapping.clear();
                        section_changed = true;
                    }
                    ui.localized_label(
                        RichText::new("支持 SourceKey=TargetKey 或 SourceKey=command:ID。")
                            .size(10.5)
                            .color(palette.muted),
                    );
                } else {
                    ui.localized_label(
                        RichText::new("未编辑时保留 INI 中已有的按键映射。")
                            .size(11.0)
                            .color(palette.muted),
                    );
                    load_keymapping = ui
                        .add_enabled(ini_path.is_some(), localized_button("载入现有映射"))
                        .clicked();
                }
                section_changed
            });
            changed |= section_changed;
            ini_changed |= section_changed;
        }

        if load_keymapping && let Some(path) = &ini_path {
            match crate::runtime::chromeplus::keymapping_text(path) {
                Ok(mapping) => {
                    config.chrome_plus.keymapping = Some(mapping);
                    changed = true;
                }
                Err(error) => {
                    self.status_message = Some(format!("读取按键映射失败：{error}"));
                }
            }
        }
        if load_ini && let Some(path) = &ini_path {
            match crate::runtime::chromeplus::read_config_from_ini(path, &config.chrome_plus) {
                Ok(loaded) => {
                    config.chrome_plus = loaded;
                    changed = true;
                }
                Err(error) => {
                    self.status_message = Some(format!("读取 Chrome++ 配置失败：{error}"));
                }
            }
        }
        if open_ini_folder
            && let Some(path) = &ini_path
            && let Err(error) = std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", path.display()))
                .spawn()
        {
            pending_status = Some(format!("无法打开配置文件所在目录：{error}"));
        }
        if backup_ini && let (Some(path), Some(directory)) = (&ini_path, &backup_dir) {
            pending_status = Some(
                match crate::runtime::chromeplus::backup_ini(path, directory, "manual") {
                    Ok(backup) => format!("INI 已备份：{}", backup.display()),
                    Err(error) => format!("备份 INI 失败：{error}"),
                },
            );
        }
        if restore_defaults && let (Some(path), Some(directory)) = (&ini_path, &backup_dir) {
            pending_status = Some(
                match crate::runtime::chromeplus::restore_default_config(path, directory) {
                    Ok(backup) => {
                        config.chrome_plus = crate::domain::runtime::ChromePlusConfig::default();
                        changed = true;
                        format!("已恢复默认配置；原文件备份到 {}", backup.display())
                    }
                    Err(error) => format!("恢复默认配置失败：{error}"),
                },
            );
        }
        if ini_changed
            && mode == ChromePlusMode::Full
            && let Some(path) = &ini_path
        {
            self.status_message = Some(
                match self
                    .state
                    .update_chromeplus_config(target, config.clone(), path.clone())
                {
                    Ok(()) => "Chrome++ 配置已自动写入".to_owned(),
                    Err(error) => format!("{}：{error}", localized("Chrome++ 配置未应用，可重试")),
                },
            );
        } else if changed {
            self.update_target_config(target, config.clone());
        }
        if let Some(message) = pending_status {
            self.status_message = Some(message);
        }
        if open_ini
            && let Some(path) = &ini_path
            && let Err(error) = std::process::Command::new("notepad.exe").arg(path).spawn()
        {
            self.status_message = Some(format!("无法打开配置文件：{error}"));
        }
        if request_update && let Some(latest) = latest_version {
            self.request_chrome_plus_update(target, installed_version, latest);
        }
        if check_update {
            self.start_operation(OperationMode::ChromePlusCheck, false);
        }
    }
}
