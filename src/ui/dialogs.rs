//! Modal dialogs and confirmation prompts for Browser Harbor.

use super::*;
use eframe::egui::{Align, CornerRadius, Frame, Layout, Stroke};

pub fn show_dialogs(app: &mut BrowserHarborApp, ctx: &egui::Context) {
    show_config_rebuild_prompt(app, ctx);
    show_channel_mismatch_prompt(app, ctx);
    show_same_version_prompt(app, ctx);
    show_chrome_plus_update_prompt(app, ctx);
    show_browser_upgrade_prompt(app, ctx);
    show_identity_prompt(app, ctx);
    show_health_dialog(app, ctx);
}

fn show_config_rebuild_prompt(app: &mut BrowserHarborApp, ctx: &egui::Context) {
    if !app.rebuilding_config {
        return;
    }
    let mut confirm = false;
    let mut cancel = false;
    let response = egui::Modal::new(egui::Id::unique("config_rebuild_confirmation")).show(ctx, |ui| {
        ui.set_width(390.0);
        ui.heading(localized("重建管理器配置"));
        ui.localized_label("将先备份原 BrowserHarbor.json，再重建默认管理器设置。浏览器程序、Profile 和用户数据不会修改。");
        ui.add(egui::Label::new(app.state.config_path().display().to_string()).wrap());
        ui.horizontal(|ui| {
            cancel = ui.localized_button("返回").clicked();
            confirm = ui.localized_button("确认备份并重建").clicked();
        });
    });
    if response.should_close() {
        cancel = true;
    }
    if confirm {
        match app.state.rebuild_manager_config() {
            Ok(backup) => {
                app.startup_error = None;
                app.rebuilding_config = false;
                app.status_message = Some(format!(
                    "{}：{}",
                    localized("管理器配置已重建，原文件备份到"),
                    backup.display()
                ));
                app.local_versions.clear();
                app.chrome_plus_versions.clear();
                app.chrome_plus_ini_loaded.clear();
                app.refresh_local_version(app.state.selected_target());
                app.refresh_program_identity(app.state.selected_target());
            }
            Err(error) => {
                app.startup_error = Some(format!("{}：{error}", localized("备份或重建配置失败")));
            }
        }
    } else if cancel {
        app.rebuilding_config = false;
    }
}

fn show_health_dialog(app: &mut BrowserHarborApp, ctx: &egui::Context) {
    let Some(dialog) = app.health_dialog.clone() else {
        return;
    };
    let palette = match ctx.theme() {
        egui::Theme::Light => LIGHT_PALETTE,
        egui::Theme::Dark => DARK_PALETTE,
    };
    let mut close = false;
    let mut relocate = false;
    let mut apply_relocation = false;
    let mut cancel_relocation = false;
    let mut request_repair = false;
    let mut apply_repair = false;
    let mut cancel_repair = false;
    egui::Modal::new(egui::Id::unique("configuration_health"))
        .frame(
            Frame::new()
                .fill(palette.surface)
                .stroke(Stroke::new(1.0, palette.border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(16),
        )
        .show(ctx, |ui| {
            ui.set_width(430.0);
            ui.heading("检查配置");
            ui.localized_label(
                RichText::new(dialog.target.display_name())
                    .size(11.0)
                    .color(palette.muted),
            );
            ui.localized_label(
                RichText::new(format!(
                    "检查结果：{}",
                    localized(health_state_label(dialog.report.state))
                ))
                .strong()
                .color(health_state_color(dialog.report.state, palette)),
            );
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("configuration_health_items")
                .max_height(230.0)
                .show(ui, |ui| {
                    for item in dialog.report.items.iter().filter(|item| {
                        item.aspect != HealthAspect::Profile
                            || dialog.target.spec().layout.data
                                != dialog.target.spec().layout.profile
                    }) {
                        ui.horizontal_wrapped(|ui| {
                            ui.localized_label(
                                RichText::new(health_aspect_label(item.aspect))
                                    .strong()
                                    .color(palette.text),
                            );
                            ui.localized_label(
                                RichText::new(health_state_label(item.state))
                                    .color(health_state_color(item.state, palette)),
                            );
                            ui.localized_label(RichText::new(&item.detail).color(palette.muted));
                        });
                    }
                });
            let repair_label = if dialog.target.chrome_plus_mode() != ChromePlusMode::Disabled {
                Some("修复 Chrome++ 路径")
            } else if matches!(
                dialog.target,
                TargetId::Firefox | TargetId::Zen | TargetId::Iceweasel | TargetId::SkyKakapo
            ) {
                Some("修复便携配置")
            } else {
                None
            };
            let repairing_portable = dialog.target.chrome_plus_mode() == ChromePlusMode::Disabled;
            let can_repair = repair_label.is_some()
                && dialog.report.items.iter().any(|item| {
                    item.aspect == HealthAspect::PortableConfig
                        && item.state >= HealthState::Warning
                });
            if dialog.confirming_repair {
                ui.separator();
                ui.localized_label(
                    RichText::new(format!(
                        "将备份 {} 并只修复路径键。",
                        if dialog.target == TargetId::SkyKakapo {
                            "tmemutil.ini"
                        } else if repairing_portable {
                            "portable.ini"
                        } else {
                            "chrome++.ini"
                        }
                    ))
                    .strong()
                    .color(palette.warning),
                );
                ui.localized_label(
                    RichText::new(if repairing_portable {
                        "请先关闭浏览器；其他便携参数和注释会保留。"
                    } else {
                        "请先关闭浏览器；其他 Chrome++ 参数和注释会保留。"
                    })
                    .size(11.0)
                    .color(palette.muted),
                );
                ui.horizontal(|ui| {
                    if ui.localized_button("取消修复").clicked() {
                        cancel_repair = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.localized_button("确认修复").clicked() {
                            apply_repair = true;
                        }
                    });
                });
            } else if can_repair
                && repair_label.is_some_and(|label| ui.localized_button(label).clicked())
            {
                request_repair = true;
            }
            if let Some(pending) = &dialog.pending_relocation {
                ui.separator();
                ui.localized_label(RichText::new("将应用以下路径").strong().color(palette.text));
                for (label, path) in [
                    ("浏览器根目录", pending.browser_root.as_deref()),
                    ("程序目录", pending.browser_directory.as_deref()),
                    ("用户数据目录", pending.data_directory.as_deref()),
                    (
                        "Profile 位置",
                        (dialog.target.spec().layout.data != dialog.target.spec().layout.profile)
                            .then_some(pending.profile_directory.as_deref())
                            .flatten(),
                    ),
                    ("缓存目录", pending.cache_directory.as_deref()),
                ] {
                    if let Some(path) = path {
                        ui.localized_label(
                            RichText::new(format!("{label}：{}", path.display()))
                                .size(11.0)
                                .color(palette.muted),
                        );
                    }
                }
                if let Some(warning) = &dialog.relocation_warning {
                    ui.localized_label(RichText::new(warning).size(11.0).color(palette.warning));
                }
                ui.horizontal(|ui| {
                    if ui.localized_button("取消重新定位").clicked() {
                        cancel_relocation = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.localized_button("应用").clicked() {
                            apply_relocation = true;
                        }
                    });
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.localized_button("关闭").clicked() {
                    close = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.localized_button("重新定位…").clicked() {
                        relocate = true;
                    }
                });
            });
        });

    if close {
        app.health_dialog = None;
    }
    if cancel_relocation && let Some(dialog) = &mut app.health_dialog {
        dialog.pending_relocation = None;
        dialog.relocation_warning = None;
    }
    if request_repair && let Some(dialog) = &mut app.health_dialog {
        dialog.confirming_repair = true;
    }
    if cancel_repair && let Some(dialog) = &mut app.health_dialog {
        dialog.confirming_repair = false;
    }
    if apply_repair {
        let target = app.state.selected_target();
        if target.chrome_plus_mode() == ChromePlusMode::Disabled {
            app.repair_current_portable_paths();
        } else {
            app.repair_current_chromeplus_paths();
        }
    }
    if apply_relocation {
        app.apply_staged_relocation();
    }
    if relocate {
        let target = app.state.selected_target();
        let current = app.state.draft().targets[&target]
            .browser_root
            .as_deref()
            .and_then(|path| resolve_preview_path(app.state.tool_root(), path));
        if let Some(root) =
            folder_picker::pick_folder(&localized("选择移动后的浏览器根目录"), current.as_deref())
        {
            app.stage_relocation(root);
        }
    }
}

fn show_same_version_prompt(app: &mut BrowserHarborApp, ctx: &egui::Context) {
    let Some(prompt) = app.same_version_prompt.clone() else {
        return;
    };
    let palette = match ctx.theme() {
        egui::Theme::Light => LIGHT_PALETTE,
        egui::Theme::Dark => DARK_PALETTE,
    };
    let mut confirm = false;
    let mut cancel = false;
    let downgrade = compare_versions(&prompt.current_version, &prompt.upstream.version)
        == VersionRelation::UpstreamOlder;
    let catsxp_unknown = app.state.selected_target() == TargetId::Catsxp
        && app.catsxp_observation.is_none()
        && crate::services::catsxp::base_version(&prompt.upstream.version, prompt.upstream.channel)
            .as_deref()
            == Some(prompt.current_version.as_str());
    let response = egui::Modal::new(egui::Id::unique("same_version_confirmation"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(390.0);
                ui.heading(localized(if downgrade { "确认降级浏览器" } else if catsxp_unknown { "确认替换 Catsxp" } else { "版本与上游相同" }));
                ui.add_space(7.0);
                egui::Grid::new("same_version_meta")
                    .num_columns(2)
                    .spacing([14.0, 6.0])
                    .show(ui, |ui| {
                        ui.localized_label("当前版本");
                        ui.localized_label(&prompt.current_version);
                        ui.end_row();
                        ui.localized_label(match prompt.upstream.source {
                            MetadataSource::Official => "上游版本",
                            MetadataSource::SignedManifest => "签名发布清单",
                            MetadataSource::SignedCache => "已缓存签名快照",
                            MetadataSource::Embedded => "内置签名快照",
                        });
                        ui.localized_label(format!(
                            "{}（{}）",
                            prompt.upstream.version,
                            localized(checked_age(prompt.upstream.checked_at))
                        ));
                        ui.end_row();
                    });
                ui.add_space(8.0);
                if catsxp_unknown {
                    ui.localized_label("当前 EXE 的基础版本相同，但无法确认渠道或 Beta 修订。继续将安装当前选择的渠道和修订。");
                }
                if downgrade {
                    ui.localized_label(RichText::new(format!("{} → {}", prompt.current_version, prompt.upstream.version)).strong().color(palette.warning));
                    ui.localized_label(RichText::new("旧版浏览器可能无法读取已由新版迁移的用户数据。请先备份数据，再确认降级。").color(palette.warning));
                }
                ui.localized_label(
                    RichText::new(
                        "继续将重新下载并替换浏览器程序目录。用户数据、缓存和可按规则恢复的 Chrome++ 配置会保留。",
                    )
                    .color(palette.muted),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    confirm = ui.localized_button(if downgrade { "确认降级" } else { "继续替换" }).clicked();
                    cancel = ui.localized_button("保留当前版本").clicked();
                });
            });
    if response.should_close() {
        cancel = true;
    }
    if confirm {
        app.same_version_prompt = None;
        app.start_operation_checked_with_expected(
            OperationMode::Install,
            true,
            false,
            None,
            prompt.channel_change_confirmed,
        );
    } else if cancel {
        app.same_version_prompt = None;
        app.operation_phase = None;
        app.status_message = Some("已取消同版本替换".to_owned());
    }
}

fn show_chrome_plus_update_prompt(app: &mut BrowserHarborApp, ctx: &egui::Context) {
    let Some(prompt) = app.chrome_plus_update_prompt.clone() else {
        return;
    };
    if prompt.target != app.state.selected_target() {
        app.chrome_plus_update_prompt = None;
        return;
    }
    let palette = match ctx.theme() {
        egui::Theme::Light => LIGHT_PALETTE,
        egui::Theme::Dark => DARK_PALETTE,
    };
    let mut confirm = false;
    let mut cancel = false;
    let response = egui::Modal::new(egui::Id::unique("chromeplus_update_confirmation"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(390.0);
                ui.heading("更新 Chrome++");
                ui.add_space(7.0);
                ui.localized_label(
                    RichText::new(format!(
                        "Chrome++ {} → {}",
                        prompt.current_version.as_deref().unwrap_or("—"),
                        prompt.latest_version
                    ))
                    .strong()
                    .color(palette.text),
                );
                ui.localized_label(format!("当前浏览器：{}", prompt.target.display_name()));
                ui.add_space(8.0);
                ui.localized_label(
                    RichText::new(
                        "将只替换 Chrome++ 文件，保留浏览器程序、用户数据和缓存；旧 INI 会先备份。",
                    )
                    .color(palette.muted),
                );
                ui.localized_label(
                    RichText::new(
                        "请保持所有当前浏览器实例关闭。点击更新后会再次检测；仍在运行的实例不会被强制结束。",
                    )
                    .color(palette.warning),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    confirm = ui.localized_button("更新 Chrome++").clicked();
                    cancel = ui.localized_button("取消").clicked();
                });
            });
    if response.should_close() {
        cancel = true;
    }
    if confirm {
        app.confirm_chrome_plus_update();
    } else if cancel {
        app.chrome_plus_update_prompt = None;
        app.status_message = Some("已取消 Chrome++ 更新".to_owned());
    }
}

fn show_browser_upgrade_prompt(app: &mut BrowserHarborApp, ctx: &egui::Context) {
    let Some(prompt) = app.browser_upgrade_prompt.clone() else {
        return;
    };
    if prompt.target != app.state.selected_target() {
        app.browser_upgrade_prompt = None;
        return;
    }
    let palette = match ctx.theme() {
        egui::Theme::Light => LIGHT_PALETTE,
        egui::Theme::Dark => DARK_PALETTE,
    };
    let mut confirm = false;
    let mut cancel = false;
    let response = egui::Modal::new(egui::Id::unique("browser_upgrade_confirmation"))
        .frame(
            Frame::new()
                .fill(palette.surface)
                .stroke(Stroke::new(1.0, palette.border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(16),
        )
        .show(ctx, |ui| {
            ui.set_width(390.0);
            ui.heading("同时更新 Chrome++");
            ui.add_space(7.0);
            ui.localized_label(format!(
                "浏览器：{} → {}",
                prompt.current_browser, prompt.upstream_browser
            ));
            ui.localized_label(
                RichText::new(format!(
                    "Chrome++：{} → {}",
                    prompt.chrome_plus_current, prompt.chrome_plus_latest
                ))
                .strong()
                .color(palette.accent_text),
            );
            ui.add_space(8.0);
            ui.localized_label(
                RichText::new(if prompt.replace_same_version {
                    "浏览器版本相同，继续会重新下载并替换浏览器，同时更新 Chrome++。"
                } else {
                    "浏览器升级会替换程序目录，因此本次操作也会安装已检测到的 Chrome++ 新版本。"
                })
                .color(palette.muted),
            );
            ui.localized_label(
                RichText::new(
                    "用户数据、缓存和可恢复的 Chrome++ 配置会保留；请先关闭当前浏览器的所有实例。",
                )
                .color(palette.warning),
            );
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                confirm = ui.localized_button("继续浏览器操作").clicked();
                cancel = ui.localized_button("取消").clicked();
            });
        });
    if response.should_close() {
        cancel = true;
    }
    if confirm {
        app.browser_upgrade_prompt = None;
        app.start_operation_checked_with_expected(
            OperationMode::Install,
            prompt.replace_same_version,
            false,
            Some(prompt.chrome_plus_latest),
            false,
        );
    } else if cancel {
        app.browser_upgrade_prompt = None;
        app.status_message = Some("已取消浏览器操作".to_owned());
    }
}

fn show_identity_prompt(app: &mut BrowserHarborApp, ctx: &egui::Context) {
    let Some(prompt) = app.identity_prompt.clone() else {
        return;
    };
    if prompt.target != app.state.selected_target() {
        app.identity_prompt = None;
        return;
    }
    let palette = match ctx.theme() {
        egui::Theme::Light => LIGHT_PALETTE,
        egui::Theme::Dark => DARK_PALETTE,
    };
    let mut confirm = false;
    let mut cancel = false;
    let response = egui::Modal::new(egui::Id::unique("browser_identity_confirmation"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(390.0);
                ui.heading("浏览器身份不一致");
                ui.add_space(7.0);
                ui.localized_label(format!(
                    "当前目标：{}",
                    prompt.target.display_name()
                ));
                ui.localized_label(format!(
                    "目录中的程序：{}（{}）",
                    prompt.actual.display_name(),
                    prompt.product_name
                ));
                ui.add_space(8.0);
                ui.localized_label(
                    RichText::new(
                        "继续安装或替换可能覆盖另一个浏览器的程序文件。建议重新选择目录；只有确认这是自定义构建时才继续。",
                    )
                    .color(palette.warning),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    confirm = ui.localized_button("仍然继续").clicked();
                    cancel = ui.localized_button("重新选择目录").clicked();
                });
            });
    if response.should_close() {
        cancel = true;
    }
    if confirm {
        app.identity_prompt = None;
        app.start_operation_checked(OperationMode::Install, prompt.replace_same_version, true);
    } else if cancel {
        app.identity_prompt = None;
        app.operation_phase = None;
        app.status_message = Some("已取消浏览器安装".to_owned());
    }
}

fn show_channel_mismatch_prompt(app: &mut BrowserHarborApp, ctx: &egui::Context) {
    let Some(prompt) = app.channel_mismatch_prompt.clone() else {
        return;
    };
    if prompt.target != app.state.selected_target() {
        app.channel_mismatch_prompt = None;
        return;
    }
    let palette = match ctx.theme() {
        egui::Theme::Light => LIGHT_PALETTE,
        egui::Theme::Dark => DARK_PALETTE,
    };
    let mut confirm = false;
    let mut cancel = false;
    let response = egui::Modal::new(egui::Id::unique("channel_mismatch_confirmation"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(430.0);
                ui.heading("渠道与现有安装不一致");
                ui.add_space(7.0);
                ui.localized_label(format!(
                    "当前安装：{} {} {}",
                    prompt.target.display_name(),
                    prompt.current_channel,
                    prompt.local_version
                ));
                if !prompt.product_name.is_empty() {
                    ui.localized_label(format!("程序标识：{}", prompt.product_name));
                }
                ui.localized_label(format!("本次选择：{}", prompt.selected_channel.display_name()));
                ui.add_space(8.0);
                ui.localized_label(
                    RichText::new(
                        "继续会替换当前程序目录，但保留用户数据和缓存。若要并行保留两个渠道，请先修改 Browser Root。",
                    )
                    .color(palette.warning),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    confirm = ui.localized_button("仍然替换").clicked();
                    cancel = ui.localized_button("返回修改").clicked();
                });
            });
    if response.should_close() {
        cancel = true;
    }
    if confirm {
        app.channel_mismatch_prompt = None;
        app.start_operation_checked_with_expected(OperationMode::Install, false, false, None, true);
    } else if cancel {
        app.channel_mismatch_prompt = None;
        app.operation_phase = None;
        app.status_message = Some("已取消渠道切换".to_owned());
    }
}
