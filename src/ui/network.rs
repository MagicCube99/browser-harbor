//! Global network and manager configuration settings panel.

use super::*;
use eframe::egui::{Align, CornerRadius, Frame, Layout, Margin, Stroke};

impl BrowserHarborApp {
    pub(super) fn show_network(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 3.0;
        let palette = palette(ui);
        let cache_usage = self.state.download_cache_usage().ok();
        let mut network = self.state.draft().network.clone();
        let mut accelerator_input = std::mem::take(&mut self.accelerator_input);
        let mut changed = false;
        let mut add_requested = false;
        let mut validation_error = None;
        let mut clear_cache = false;
        let mut requested_config_location = None;
        let mut show_local_package = self.state.draft().show_local_package;
        let mut auto_check_metadata = self.state.draft().auto_check_metadata;
        let mut use_signed_metadata_cache = self.state.draft().use_signed_metadata_cache;
        let mut refresh_metadata = false;

        changed |= section(ui, "下载加速", |ui| {
            let mut section_changed = ui
                .localized_checkbox(&mut network.accelerators_enabled, "启用内置下载加速站")
                .changed();
            ui.localized_label(
                RichText::new("添加额外的自定义加速站（可选）")
                    .size(11.0)
                    .color(palette.muted),
            );
            ui.horizontal(|ui| {
                let label = field_label(ui, "加速站前缀", FIELD_LABEL_WIDTH)
                    .localized_hover("加速站 URL 前缀；程序会在原始下载地址前拼接此文本");
                let response = ui
                    .add_sized(
                        [(ui.available_width() - 52.0).max(80.0), 24.0],
                        egui::TextEdit::singleline(&mut accelerator_input)
                            .hint_text("https://example.com/"),
                    )
                    .labelled_by(label.id);
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    add_requested = true;
                }
                add_requested |= ui
                    .add_sized([46.0, 24.0], localized_button("添加"))
                    .clicked();
            });
            let mut remove = None;
            let mut move_up = None;
            let mut move_down = None;
            for index in 0..network.accelerators.len() {
                let prefix = network.accelerators[index].prefix.clone();
                ui.horizontal(|ui| {
                    ui.localized_checkbox(
                        &mut network.accelerators[index].enabled,
                        RichText::new(prefix).size(11.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.localized_button("删除").clicked() {
                            remove = Some(index);
                        }
                        if ui
                            .add_enabled(
                                index + 1 < network.accelerators.len(),
                                localized_button("下移"),
                            )
                            .clicked()
                        {
                            move_down = Some(index);
                        }
                        if ui
                            .add_enabled(index > 0, localized_button("上移"))
                            .clicked()
                        {
                            move_up = Some(index);
                        }
                    });
                });
            }
            if let Some(index) = remove {
                network.accelerators.remove(index);
                section_changed = true;
            } else if let Some(index) = move_up {
                network.accelerators.swap(index, index - 1);
                section_changed = true;
            } else if let Some(index) = move_down {
                network.accelerators.swap(index, index + 1);
                section_changed = true;
            }
            if network.accelerators.is_empty() {
                ui.localized_label(
                    RichText::new("未添加自定义加速站；仍会尝试内置 GitHub 加速入口。")
                        .size(11.0)
                        .color(palette.muted),
                );
            }
            ui.localized_label(
                RichText::new("加速站只改变 GitHub 传输路径，不改变版本、长度或摘要校验。")
                    .size(11.0)
                    .color(palette.muted),
            );
            section_changed
        });

        if add_requested {
            let accelerator = crate::domain::settings::Accelerator {
                prefix: accelerator_input.trim().to_owned(),
                enabled: true,
            };
            if network.accelerators.len() >= 20 {
                validation_error = Some("最多添加 20 个加速站".to_owned());
            } else if let Err(error) = accelerator.validate() {
                validation_error = Some(error);
            } else if network.accelerators.iter().any(|existing| {
                existing
                    .prefix
                    .trim()
                    .eq_ignore_ascii_case(&accelerator.prefix)
            }) {
                validation_error = Some("加速站已经存在".to_owned());
            } else {
                network.accelerators.push(accelerator);
                accelerator_input.clear();
                changed = true;
            }
        }
        if let Some(error) = &validation_error {
            ui.colored_label(palette.error, localized(error));
        }

        ui.add_space(7.0);
        let mut proxy_changed = false;
        ui.columns(2, |columns| {
            let mut timeout_changed = false;
            proxy_changed = Frame::new()
                .fill(palette.surface)
                .stroke(Stroke::new(1.0, palette.border))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(9, 4))
                .show(&mut columns[0], |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (title_rect, _) = ui.allocate_exact_size(
                            egui::vec2(FIELD_LABEL_WIDTH, 24.0),
                            egui::Sense::hover(),
                        );
                        let mut title_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(title_rect)
                                .layout(Layout::left_to_right(Align::Center)),
                        );
                        title_ui.localized_label(
                            RichText::new("代理与超时")
                                .size(12.0)
                                .strong()
                                .color(palette.text),
                        );
                        let timeout = ui
                            .add_sized(
                                [76.0, 24.0],
                                egui::DragValue::new(&mut network.timeout_seconds)
                                    .range(5..=300)
                                    .speed(1.0)
                                    .suffix(format!(" {}", localized("秒"))),
                            )
                            .on_hover_text(localized("代理请求超时（秒）"));
                        timeout.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::SpinButton,
                                true,
                                localized("超时"),
                            )
                        });
                        timeout_changed = timeout.changed();
                    });
                    ui.add_space(1.0);
                    let mut section_changed = false;
                    let mut proxy = network.proxy.clone().unwrap_or_default();
                    let changed =
                        text_field(ui, "代理地址", &mut proxy, "留空时使用系统代理").changed();
                    if changed {
                        network.proxy = (!proxy.trim().is_empty()).then(|| proxy.trim().to_owned());
                        section_changed = true;
                    }
                    section_changed
                })
                .inner;
            proxy_changed |= timeout_changed;

            section_with_header(
                &mut columns[1],
                "下载缓存",
                |_| {},
                |ui| {
                    let usage = cache_usage.map_or_else(
                        || "无法读取缓存目录".to_owned(),
                        |usage| {
                            format!(
                                "{} {}，{}",
                                usage.files,
                                localized("个文件"),
                                format_bytes(usage.bytes)
                            )
                        },
                    );
                    egui::Sides::new()
                        .height(24.0)
                        .spacing(6.0)
                        .shrink_left()
                        .truncate()
                        .show(
                            ui,
                            |ui| {
                                ui.localized_label(
                                    RichText::new(&usage).size(11.0).color(palette.muted),
                                )
                                .localized_hover(&usage);
                            },
                            |ui| {
                                let detail = format!(
                                    "{}\n{}",
                                    self.state.cache_dir().display(),
                                    localized("仅删除可重新下载的安装包，不影响浏览器数据。")
                                );
                                clear_cache = ui
                                    .add_enabled(
                                        self.operation.is_none()
                                            && cache_usage.is_some_and(|usage| usage.files > 0),
                                        localized_button("清理下载缓存"),
                                    )
                                    .on_hover_text(detail)
                                    .clicked();
                            },
                        );
                },
            );
        });
        changed |= proxy_changed;

        ui.add_space(7.0);
        section(ui, "配置位置", |ui| {
            let mut portable = self.state.is_portable_config();
            ui.horizontal(|ui| {
                let user = ui.radio_value(&mut portable, false, localized("系统用户目录（默认）"));
                let user_changed = user.changed();
                user.localized_hover("保存在 %LOCALAPPDATA%\\browser-harbor\\BrowserHarbor.json");
                let portable_response =
                    ui.radio_value(&mut portable, true, localized("程序目录（便携）"));
                let portable_changed = portable_response.changed();
                portable_response.localized_hover(
                    "将 BrowserHarbor.json 保存在 Browser Harbor EXE 旁，可随程序一起带走",
                );
                if user_changed || portable_changed {
                    requested_config_location = Some(portable);
                }
            });
            ui.horizontal(|ui| {
                ui.localized_label(
                    RichText::new("当前配置文件")
                        .size(10.5)
                        .color(palette.muted),
                );
                let path = self.state.config_path().display().to_string();
                ui.add(
                    egui::Label::new(
                        RichText::new(&path)
                            .size(10.5)
                            .monospace()
                            .color(palette.text),
                    )
                    .truncate(),
                )
                .localized_hover(path);
            });
            ui.localized_label(
                RichText::new(
                    "便携模式要求 EXE 所在目录可写（避免放在 C 盘根目录或 Program Files 等受保护位置）；浏览器绝对路径在新电脑上仍需重新定位。",
                )
                .size(10.5)
                .color(palette.muted),
            );
        });

        ui.add_space(5.0);
        let mut advanced_changed = false;
        egui::CollapsingHeader::new(localized("高级"))
            .id_salt("advanced_settings")
            .show(ui, |ui| {
                advanced_changed =
                    ui.localized_checkbox(&mut auto_check_metadata, "自动检查校验信息")
                        .changed();
                ui.localized_label(
                    RichText::new("进入浏览器页面时自动检查所选浏览器的官方版本和签名校验信息。")
                        .size(10.5)
                        .color(palette.muted),
                );
                ui.add_space(3.0);
                advanced_changed |=
                    ui.localized_checkbox(
                        &mut use_signed_metadata_cache,
                        "离线时使用最近一次已签名的校验信息",
                    )
                    .changed();
                ui.localized_label(
                    RichText::new("实时校验信息不可用时使用最近一次已验签缓存；签名、大小和摘要仍会校验。")
                        .size(10.5)
                        .color(palette.muted),
                );
                ui.add_space(3.0);
                refresh_metadata = ui
                    .add_enabled(
                        self.operation.is_none(),
                        localized_button("立即重新获取校验信息"),
                    )
                    .clicked();
                ui.add_space(3.0);
                advanced_changed = ui.localized_checkbox(&mut show_local_package, "使用本地包替代下载").changed()
                    || advanced_changed;
                ui.localized_label(RichText::new("仅替代当前发布包的下载，不支持导入其他版本。仍会校验大小、摘要和必要的签名，并可能需要联网。关闭会清除选择记录，不删除文件。").size(10.5).color(palette.muted));
            });
        self.accelerator_input = accelerator_input;
        if clear_cache {
            self.status_message = Some(match self.state.clear_download_cache() {
                Ok(removed) => {
                    format!("已清理下载缓存：{}", format_bytes(removed.bytes))
                }
                Err(error) => format!("清理下载缓存失败：{error}"),
            });
        }
        if changed || advanced_changed {
            self.status_message = None;
            self.operation_phase = None;
            self.state.update_draft(|settings| {
                settings.network = network;
                settings.show_local_package = show_local_package;
                settings.auto_check_metadata = auto_check_metadata;
                settings.use_signed_metadata_cache = use_signed_metadata_cache;
                if !show_local_package {
                    for config in settings.targets.values_mut() {
                        config.local_package = None;
                    }
                }
            });
            self.auto_save();
        }
        if refresh_metadata && self.operation.is_none() {
            self.start_operation(OperationMode::MetadataRefresh, false);
        }
        if let Some(portable) = requested_config_location
            && self.startup_error.is_none()
        {
            match self.state.switch_config_location(portable) {
                Ok(()) => {
                    self.status_message = Some(format!(
                        "{}：{}",
                        localized("配置位置已切换"),
                        self.state.config_path().display()
                    ));
                }
                Err(error) => {
                    let message = if error.is_permission_denied() {
                        localized(
                            "切换配置位置失败：当前目录受系统权限保护（如 C 盘根目录或 Program Files）。请将程序移至普通用户目录（如 D 盘或 C:\\Tools），或右键以管理员身份运行。",
                        )
                    } else {
                        format!("{}：{error}", localized("切换配置位置失败"))
                    };
                    self.status_message = Some(message);
                }
            }
        }
    }
}
