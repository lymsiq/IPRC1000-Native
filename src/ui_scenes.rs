//! APP 场景管理界面（P0）
//!
//! 一个场景 = 一个启动项（程序）+ 启动后要切到的配置组。
//! 列表里每行显示场景名、绑定的程序、目标组；「新增/编辑」用模态弹窗。
//! 所有写盘都走 `app_scenes::save`，本模块只负责收集输入与显示结果。

use egui::{Color32, RichText};
use std::sync::Mutex;

use crate::{app_scenes, region_learn, settings, shortcut_store};

/// 常用语编辑弹窗状态（模块内自持，避免改调用方 App 结构）。
/// 4 组 = 文本轮盘的 ↑→↓← 四个扇区。
struct PhraseEdit {
    num: u32,
    rows: [String; 4],
}
static PHRASE_EDIT: Mutex<Option<PhraseEdit>> = Mutex::new(None);

/// 编辑弹窗状态。num == 0 表示新建。
pub struct SceneEdit {
    pub num: u32,
    pub name: String,
    /// 启动项编号（下拉里选中的 shortcut #）
    pub shortcut_num: u32,
    pub group: u8,
}

impl SceneEdit {
    pub fn new() -> Self {
        Self {
            num: 0,
            name: String::new(),
            shortcut_num: 0,
            group: settings::active_group(),
        }
    }

    pub fn from_scene(s: &app_scenes::Scene) -> Self {
        Self {
            num: s.num,
            name: s.name.clone(),
            shortcut_num: s.shortcut_num,
            group: s.group,
        }
    }
}

/// 渲染场景列表页。`edit` 由调用方持有（模态弹窗在列表之外画）。
/// 返回 Some(消息) 表示有操作要提示。
pub fn page(ui: &mut egui::Ui, edit: &mut Option<SceneEdit>) -> Option<String> {
    let mut msg = None;

    ui.horizontal(|ui| {
        ui.label(RichText::new("APP 场景").heading().strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("＋ 新增场景").clicked() {
                *edit = Some(SceneEdit::new());
            }
        });
    });
    ui.label(
        RichText::new("按键一键启动程序后，自动切到该场景绑定的配置组（如 WorkBuddy 快捷键）。")
            .small()
            .weak(),
    );
    ui.label(
        RichText::new("学完锚点后：启动会自动聚焦输入框；翻页需在「按键映射」把上下键绑成「场景翻页↑/↓」。")
            .small()
            .weak(),
    );
    ui.separator();

    // ★ 学习状态条（2026-11-20）：学习时窗口可以切走，状态在这里持续可见。
    match region_learn::state() {
        region_learn::LearnState::Countdown(sec) => {
            ui.label(RichText::new(format!(
                "准备学习输入框：{sec} 秒后开始监听 —— 请切到目标程序，把鼠标放到输入框上（不要点）"
            )).color(Color32::from_rgb(230, 170, 90)));
            if ui.button("取消学习").clicked() { region_learn::cancel(); }
        }
        region_learn::LearnState::Waiting(sec) => {
            ui.label(RichText::new(format!(
                "正在监听点击（已等 {sec}s，60s 超时）—— 在目标程序的输入框里点一下"
            )).color(Color32::from_rgb(120, 200, 240)));
            if ui.button("取消学习").clicked() { region_learn::cancel(); }
        }
        region_learn::LearnState::Captured { rx, ry, window } => {
            ui.label(RichText::new(format!(
                "√ 已学习：窗口「{window}」 位置 ({:.0}%, {:.0}%) —— 已写入场景",
                rx * 100.0, ry * 100.0
            )).color(Color32::from_rgb(110, 210, 140)));
        }
        region_learn::LearnState::Idle => {}
    }

    let groups = settings::groups_ui();
    let scenes = app_scenes::all();

    if scenes.is_empty() {
        ui.add_space(12.0);
        ui.label(
            RichText::new("还没有场景。点右上角「＋ 新增场景」，选一个已绑定的程序和一个配置组。")
                .color(Color32::from_rgb(200, 170, 80)),
        );
        return msg;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for s in &scenes {
            let group_name = settings::group_name(s.group);
            let shortcut = shortcut_store::get(s.shortcut_num);
            let prog = shortcut
                .as_ref()
                .map(|x| if x.name.trim().is_empty() { x.path.clone() } else { x.name.clone() })
                .unwrap_or_else(|| format!("启动项 #{}（未配置）", s.shortcut_num));

            egui::Frame::none()
                .inner_margin(egui::vec2(8.0, 6.0))
                .stroke(egui::Stroke::new(0.5_f32, Color32::from_gray(70)))
                .rounding(egui::Rounding::same(8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&s.name).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("删除").clicked() {
                                app_scenes::delete(s.num);
                                msg = Some(format!("已删除场景「{}」", s.name));
                            }
                            if ui.button("编辑").clicked() {
                                *edit = Some(SceneEdit::from_scene(s));
                            }
                            if ui.button("常用语").clicked() {
                                let rows = if s.phrases.iter().all(|t| t.is_empty()) {
                                    app_scenes::preset_phrases()
                                } else {
                                    s.phrases.clone()
                                };
                                if let Ok(mut g) = PHRASE_EDIT.lock() {
                                    *g = Some(PhraseEdit { num: s.num, rows });
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("启动：").weak());
                        ui.label(prog);
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("切到：").weak());
                        ui.label(RichText::new(format!("配置组 {} · {}", s.group + 1, group_name)));
                    });
                    ui.horizontal(|ui| {
                        let input = if s.input_region.is_some() { "已学习" } else { "待学习" };
                        let page = if s.page_region.is_some() { "已学习" } else { "待学习" };
                        let ph_n = s.phrases.iter().filter(|t| !t.is_empty()).count();
                        let phrases = if ph_n == 0 { "未配".to_string() } else { format!("{ph_n} 句") };
                        ui.label(RichText::new(format!("输入框：{input}　翻页区：{page}　常用语：{phrases}")).small().weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let busy = region_learn::is_active();
                            if ui.add_enabled(!busy, egui::Button::new("学习翻页区").small()).clicked() {
                                region_learn::start(s.num, "page");
                                msg = Some("3 秒后开始监听，请切到目标程序点击翻页区位置".into());
                            }
                            if ui.add_enabled(!busy, egui::Button::new("学习输入框").small()).clicked() {
                                region_learn::start(s.num, "input");
                                msg = Some("3 秒后开始监听，请切到目标程序点击输入框位置".into());
                            }
                        });
                    });
                });
            ui.add_space(5.0);
        }
    });

    let _ = groups; // 组列表在弹窗里用；这里只为提前取一次长度做校验参考
    // 常用语弹窗（模块内状态自持）
    if let Some(m) = phrase_window(ui.ctx()) { msg = Some(m); }
    msg
}

/// 场景常用语编辑弹窗：给每个可绑定键配一句短语。
/// 配好后，切到本场景时这些键按下即上屏（优先于全局文本槽）。
fn phrase_window(ctx: &egui::Context) -> Option<String> {
    let mut msg = None;
    let mut open = PHRASE_EDIT.lock().map(|g| g.is_some()).unwrap_or(false);
    if !open { return None; }

    let title = "场景常用语 · 按键直发";
    let mut should_close = false;
    let mut to_save: Option<(u32, [String; 4])> = None;
    {
        let mut guard = match PHRASE_EDIT.lock() { Ok(g) => g, Err(_) => return None };
        let Some(edit) = guard.as_mut() else { return None };
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(440.0)
            .default_height(280.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new("4 组常用语 = 文本轮盘的 ↑ → ↓ ← 四个扇区。")
                    .small().weak());
                ui.label(RichText::new("用法：在本场景按「文本轮盘」键弹出圆盘 → 按对应方向键直接上屏。留空 = 该扇区用全局文本槽。")
                    .small().weak());
                ui.separator();
                for (i, arrow) in ["↑", "→", "↓", "←"].iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized([40.0, 20.0], egui::Label::new(RichText::new(*arrow).strong()));
                        ui.text_edit_singleline(&mut edit.rows[i]);
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("恢复模板").clicked() {
                        edit.rows = app_scenes::preset_phrases();
                    }
                    if ui.button("取消").clicked() { should_close = true; }
                    if ui.button("保存").clicked() {
                        to_save = Some((edit.num, edit.rows.clone()));
                        should_close = true;
                    }
                });
            });
    }
    if let Some((num, rows)) = to_save {
        match app_scenes::set_phrases(num, rows) {
            Ok(()) => msg = Some("常用语已保存".into()),
            Err(e) => msg = Some(format!("保存失败：{e}")),
        }
    }
    if !open || should_close {
        if let Ok(mut g) = PHRASE_EDIT.lock() { *g = None; }
    }
    msg
}

/// 渲染「新增/编辑」模态弹窗。返回 Some(消息) 表示发生了保存。
/// 关闭（取消 / 保存 / ×）统一由调用方清理 `edit`。
pub fn edit_window(ctx: &egui::Context, edit: &mut Option<SceneEdit>) -> Option<String> {
    let mut msg = None;
    let mut open = true;
    let title = match edit.as_ref() {
        Some(s) if s.num != 0 => "编辑 APP 场景",
        _ => "新增 APP 场景",
    };
    let mut should_close = false;
    {
        let st = edit.as_mut().expect("edit_window 只在 edit=Some 时调用");
        egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(RichText::new("场景名称").weak());
            ui.text_edit_singleline(&mut st.name);

            ui.add_space(6.0);
            ui.label(RichText::new("一键启动的程序").weak());
            let shortcuts = shortcut_store::list();
            // ★ id 必须唯一：原来两个下拉都用 from_label("")，空 label ⇒ egui 里
            //   两个 ComboBox 拿到同一个 Id，popup 状态互相串台 —— 表现就是
            //   "选了程序却没写进 shortcut_num"，保存报"请先选一个启动项（程序）"。
            //   数据里其实有启动项（shortcuts.json），纯粹是 UI 的 id 冲突。
            egui::ComboBox::from_id_salt("scene-shortcut-picker")
                .selected_text(if st.shortcut_num == 0 {
                    "— 选择程序 —".to_string()
                } else {
                    shortcut_store::label(st.shortcut_num)
                })
                .width(400.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.shortcut_num, 0, "— 选择程序 —");
                    for (i, s) in shortcuts.iter().enumerate() {
                        let num = i as u32 + 1;
                        let name = if s.name.trim().is_empty() { s.path.clone() } else { s.name.clone() };
                        ui.selectable_value(&mut st.shortcut_num, num, format!("#{num}  {name}"));
                    }
                });
            if shortcuts.is_empty() {
                ui.label(
                    RichText::new("还没有程序。请先在「按键映射」页给某个键点「程序」绑定一个。")
                        .small()
                        .color(Color32::from_rgb(230, 170, 90)),
                );
            }

            ui.add_space(6.0);
            ui.label(RichText::new("启动后切换到").weak());
            let groups = settings::groups_ui();
            egui::ComboBox::from_id_salt("scene-group-picker")
                .selected_text(format!("配置组 {} · {}", st.group + 1, settings::group_name(st.group)))
                .width(400.0)
                .show_ui(ui, |ui| {
                    for g in &groups {
                        let id = g["id"].as_u64().unwrap_or(0) as u8;
                        let name = g["name"].as_str().unwrap_or("组");
                        ui.selectable_value(&mut st.group, id, format!("配置组 {} · {}", id + 1, name));
                    }
                });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("取消").clicked() {
                    should_close = true;
                }
                if ui.button("保存").clicked() {
                    let n = settings::groups_ui().len();
                    match app_scenes::save(st.num, &st.name, st.shortcut_num, st.group, n) {
                        Ok(num) => {
                            msg = Some(format!("已保存场景「{}」", app_scenes::label(num)));
                            should_close = true;
                        }
                        Err(e) => msg = Some(format!("保存失败：{e}")),
                    }
                }
            });
        });
    } // st 的可变借用在此结束
    // 点「取消/保存」(should_close) 或窗口 × (open=false) 都清 edit
    if !open || should_close {
        *edit = None;
    }
    msg
}
