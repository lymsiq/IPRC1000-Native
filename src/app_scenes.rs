//! APP 场景（2026-11-17，P0）
//!
//! 需求：按键一键启动某个 APP 后，自动切到该 APP 专属的配置组；
//! 后续（P1）再在该组里"学习"输入框 / 翻页区锚点，做成一个可复用的场景。
//!
//! ## 为什么是独立的一张表，而不是塞进配置组
//!
//! 配置组（`settings.groups`）只描述"键 → 动作"，它不知道"哪个键会启动哪个程序"。
//! 启动项在 `shortcuts.json`（编号 1..999），绑定值里存的是 `-2000 - 编号`。
//! 所以链条是：**遥控键 → 启动项编号 → 场景 → 配置组**。
//! 用 `shortcut_num` 做场景的唯一关联键，启动后端一 detecting 到这个编号，
//! 就能查表切组 —— 不需要改绑定编码，也不需要窗口轮询（P0 阶段）。
//!
//! ## 存储
//!
//! 独立文件 `native-scenes.json`（与 C# 版、与 shortcuts/macros 分开），
//! 路径统一走 `paths::data_dir()`。编号 1 起，删除不复用（与宏一致）。
//!
//! ## 预留字段（P0 不采集、不显示，字段先落盘避免日后迁移）
//!
//! - `input_region` / `page_region`：相对窗口的归一化矩形，P1 学习锚点时填；
//! - `window_class` / `window_title`：窗口匹配线索，P2 按前台窗口自动激活时用。
//!   P0 不写也不读这两个字段，留空字符串即可 —— 不据此做任何判定，
//!   避免出现"界面显示已匹配、实际没匹配"的假仪表。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 相对窗口的归一化矩形（0.0~1.0）。P1 锚点用，P0 只预留。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Region {
    #[serde(default)]
    pub rx: f32,
    #[serde(default)]
    pub ry: f32,
    #[serde(default)]
    pub rw: f32,
    #[serde(default)]
    pub rh: f32,
}

fn default_4() -> [String; 4] {
    std::array::from_fn(|_| String::new())
}

/// 一个 APP 场景
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default)]
    pub num: u32,
    #[serde(default)]
    pub name: String,
    /// 关联的启动项编号（shortcuts.json 的 1 起编号）；0 = 未关联
    #[serde(default)]
    pub shortcut_num: u32,
    /// 启动后要切到的配置组下标
    #[serde(default)]
    pub group: u8,
    /// P1：输入框锚点。P0 恒为 None。
    #[serde(default)]
    pub input_region: Option<Region>,
    /// P1：翻页区锚点。P0 恒为 None。
    #[serde(default)]
    pub page_region: Option<Region>,
    /// P2：窗口类名线索。P0 留空。
    #[serde(default)]
    pub window_class: String,
    /// P2：窗口标题关键字。P0 留空。
    #[serde(default)]
    pub window_title: String,
    /// 场景专属常用语 4 组（2026-11-20）：对齐文本轮盘的 ↑→↓← 四个方向。
    /// 切到本场景后，按「文本轮盘」键弹出的 4 个扇区显示这 4 句，按方向键直接上屏。
    /// 空串 = 该扇区回退到全局 text_slot 对应槽（1..4）。
    #[serde(default = "default_4")]
    pub phrases: [String; 4],
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            num: 0,
            name: String::new(),
            shortcut_num: 0,
            group: 0,
            input_region: None,
            page_region: None,
            window_class: String::new(),
            window_title: String::new(),
            phrases: default_4(),
        }
    }
}

fn path() -> PathBuf {
    crate::paths::data_dir().join("native-scenes.json")
}

fn store() -> &'static Mutex<Vec<Scene>> {
    static M: OnceLock<Mutex<Vec<Scene>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(Vec::new()))
}

/// 启动时读一次磁盘
pub fn load() {
    let Ok(txt) = std::fs::read_to_string(path()) else { return };
    let Ok(v) = serde_json::from_str::<Value>(&txt) else { return };
    let Some(arr) = v["scenes"].as_array() else { return };
    let mut list = Vec::new();
    for s in arr {
        let num = s["num"].as_u64().unwrap_or(0) as u32;
        if num == 0 {
            continue;
        }
        list.push(Scene {
            num,
            name: s["name"].as_str().unwrap_or("").to_string(),
            shortcut_num: s["shortcutNum"].as_u64().unwrap_or(0) as u32,
            group: s["group"].as_u64().unwrap_or(0) as u8,
            input_region: parse_region(s.get("inputRegion")),
            page_region: parse_region(s.get("pageRegion")),
            window_class: s["windowClass"].as_str().unwrap_or("").to_string(),
            window_title: s["windowTitle"].as_str().unwrap_or("").to_string(),
            phrases: parse_phrases(s.get("phrases")),
        });
    }
    list.sort_by_key(|s| s.num);
    if let Ok(mut g) = store().lock() {
        *g = list;
    }
}

fn parse_region(v: Option<&Value>) -> Option<Region> {
    let v = v?;
    if v.is_null() {
        return None;
    }
    Some(Region {
        rx: v["rx"].as_f64().unwrap_or(0.0) as f32,
        ry: v["ry"].as_f64().unwrap_or(0.0) as f32,
        rw: v["rw"].as_f64().unwrap_or(0.0) as f32,
        rh: v["rh"].as_f64().unwrap_or(0.0) as f32,
    })
}

/// 场景常用语反序列化：{"ok":"继续","info":"说清楚点", ...}
fn parse_phrases(v: Option<&Value>) -> [String; 4] {
    let mut out = default_4();
    let Some(v) = v else { return out };
    // 新格式：4 元数组 ["继续","说清楚点","按你的来","好的"]
    if let Some(arr) = v.as_array() {
        for (i, x) in arr.iter().take(4).enumerate() {
            out[i] = x.as_str().unwrap_or("").to_string();
        }
        return out;
    }
    // 旧格式兼容：bid→text map，按固定顺序 up/right/down/left 取前 4 个
    if let Some(obj) = v.as_object() {
        for (i, bid) in ["up", "right", "down", "left"].iter().enumerate() {
            out[i] = obj.get(*bid).and_then(|x| x.as_str()).unwrap_or("").to_string();
        }
    }
    out
}

fn region_json(r: Option<Region>) -> Value {
    match r {
        None => Value::Null,
        Some(x) => json!({ "rx": x.rx, "ry": x.ry, "rw": x.rw, "rh": x.rh }),
    }
}

fn flush(list: &[Scene]) {
    let p = path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let arr: Vec<Value> = list
        .iter()
        .map(|s| {
            json!({
                "num": s.num,
                "name": s.name,
                "shortcutNum": s.shortcut_num,
                "group": s.group,
                "inputRegion": region_json(s.input_region),
                "pageRegion": region_json(s.page_region),
                "windowClass": s.window_class,
                "windowTitle": s.window_title,
                "phrases": s.phrases,
            })
        })
        .collect();
    let _ = std::fs::write(
        &p,
        serde_json::to_string_pretty(&json!({ "scenes": arr })).unwrap_or_default(),
    );
}

pub fn all() -> Vec<Scene> {
    store().lock().map(|g| g.clone()).unwrap_or_default()
}

pub fn get(num: u32) -> Option<Scene> {
    store().lock().ok().and_then(|g| g.iter().find(|s| s.num == num).cloned())
}

/// 按启动项编号反查场景。后端一键启动时用它决定切哪个组。
pub fn find_by_shortcut(shortcut_num: u32) -> Option<Scene> {
    if shortcut_num == 0 {
        return None;
    }
    store()
        .lock()
        .ok()
        .and_then(|g| g.iter().find(|s| s.shortcut_num == shortcut_num).cloned())
}

/// 纯逻辑校验：name 规整、shortcut 已关联、group 在有效范围内。
/// `group_count` 由调用方从 `settings.groups_ui().len()` 传入，
/// 这样本函数不依赖全局状态，可直接单测。
pub fn validate(
    name: &str,
    shortcut_num: u32,
    group: u8,
    group_count: usize,
) -> Result<String, String> {
    if shortcut_num == 0 {
        return Err("请先选一个启动项（程序）".into());
    }
    if group_count == 0 || group as usize >= group_count {
        return Err(format!("配置组 {group} 不存在（当前共 {group_count} 组）"));
    }
    let n = name.trim();
    Ok(if n.is_empty() {
        format!("场景 {}", shortcut_num)
    } else {
        n.to_string()
    })
}

/// 保存：`num == 0` 新建，分配 1..999 里第一个空号（删除不复用）。
/// 一个启动项只能属于一个场景 —— 重复关联直接拒绝，避免两个场景抢同一把钥匙。
pub fn save(
    num: u32,
    name: &str,
    shortcut_num: u32,
    group: u8,
    group_count: usize,
) -> Result<u32, String> {
    let name = validate(name, shortcut_num, group, group_count)?;
    let mut g = store().lock().map_err(|_| "存储忙".to_string())?;
    let final_num = if num == 0 {
        let used: std::collections::HashSet<u32> = g.iter().map(|s| s.num).collect();
        (1u32..=999).find(|n| !used.contains(n)).ok_or("场景数量已达上限 999")?
    } else {
        num
    };
    if let Some(other) = g.iter().find(|s| s.num != final_num && s.shortcut_num == shortcut_num) {
        return Err(format!(
            "启动项 #{} 已被场景「{}」占用",
            shortcut_num, other.name
        ));
    }
    if let Some(s) = g.iter_mut().find(|s| s.num == final_num) {
        s.name = name;
        s.shortcut_num = shortcut_num;
        s.group = group;
    } else {
        let mut s = Scene::default();
        s.num = final_num;
        s.name = name;
        s.shortcut_num = shortcut_num;
        s.group = group;
        g.push(s);
    }
    g.sort_by_key(|s| s.num);
    flush(&g);
    Ok(final_num)
}

pub fn delete(num: u32) {
    if let Ok(mut g) = store().lock() {
        g.retain(|s| s.num != num);
        flush(&g);
    }
}

/// 学习到的区域写回场景（2026-11-20）。
///
/// `kind`: "input" = 输入框锚点；"page" = 翻页区锚点。其它值拒绝。
/// `window`: 学习时前台窗口的标题 —— 播放端靠它确认"目标窗口真的起来了"再点，
///           绝不对着错误窗口盲点（确定事件，不是猜）。
/// 只改区域字段，不碰 name/shortcut/group —— 学习是独立的一步，
/// 不能因为"重新学了输入框"就把用户改过的名字/绑定冲掉。
pub fn set_region(num: u32, kind: &str, region: Option<Region>, window: &str) -> Result<(), String> {
    let mut g = store().lock().map_err(|_| "存储忙".to_string())?;
    let s = g.iter_mut().find(|s| s.num == num).ok_or("场景不存在")?;
    match kind {
        "input" => s.input_region = region,
        "page" => s.page_region = region,
        _ => return Err(format!("未知区域类型 {kind}")),
    }
    if !window.trim().is_empty() { s.window_title = window.trim().to_string(); }
    flush(&g);
    Ok(())
}

/// 替换某场景的 4 组常用语（2026-11-20）。索引 0..3 = 轮盘 ↑→↓←。
/// 只改 phrases，不碰其它字段。
pub fn set_phrases(num: u32, phrases: [String; 4]) -> Result<(), String> {
    let mut g = store().lock().map_err(|_| "存储忙".to_string())?;
    let s = g.iter_mut().find(|s| s.num == num).ok_or("场景不存在")?;
    s.phrases = phrases;
    flush(&g);
    Ok(())
}

/// 当前激活场景的 4 组常用语（轮盘用）。没激活场景返回 None。
pub fn active_phrases() -> Option<[String; 4]> {
    let n = active();
    if n == 0 { return None; }
    store().lock().ok()
        .and_then(|g| g.iter().find(|s| s.num == n).cloned())
        .map(|s| s.phrases)
}

/// 当前激活场景里，轮盘某扇区（0..3）的常用语；空串 = 没配（回退全局 text_slot）。
pub fn active_phrase(slot: usize) -> Option<String> {
    active_phrases().and_then(|p| p.get(slot).cloned()).filter(|t| !t.is_empty())
}

/// Vibe Coding 预置模板：轮盘 ↑→↓← 四句。
pub fn preset_phrases() -> [String; 4] {
    ["继续".into(), "说清楚一点".into(), "按你的来".into(), "好的".into()]
}

// ── 当前激活场景（2026-11-20）──
// 启动项触发场景时记下 num；切组/退出时清 0。
// 翻页动作查它，才知道该去哪个场景的 page_region 点。
static ACTIVE_SCENE: AtomicU32 = AtomicU32::new(0);
pub fn set_active(num: u32) { ACTIVE_SCENE.store(num, Ordering::SeqCst); }
pub fn active() -> u32 { ACTIVE_SCENE.load(Ordering::SeqCst) }
pub fn clear_active() { ACTIVE_SCENE.store(0, Ordering::SeqCst); }
pub fn active_scene() -> Option<Scene> {
    let n = active();
    if n == 0 { return None; }
    get(n)
}

/// 给界面的一行描述
pub fn label(num: u32) -> String {
    match get(num) {
        Some(s) => format!("{} → 组 {}", s.name, s.group + 1),
        None => format!("场景 #{num}（未配置）"),
    }
}

/// 给后端启动成功时用的日志文案（含组名，便于排查"切到哪去了"）
pub fn activation_note(num: u32) -> String {
    match get(num) {
        Some(s) => format!(
            "启动「{}」→ 场景「{}」→ 配置组 {}（{}）",
            crate::shortcut_store::label(s.shortcut_num),
            s.name,
            s.group + 1,
            crate::settings::group_name(s.group)
        ),
        None => String::new(),
    }
}

/// 落盘链路自检（纯数据层，不碰窗口/注入）。供 `--scene-selftest` 调用。
/// 输入输出都走当前 `paths::data_dir()`；调用方应先把 HOME 指到临时目录，
/// 避免污染真实配置。返回逐行报告，末行是结论。
pub fn self_test() -> String {
    let mut out = Vec::new();
    let mut ok = true;
    let mut chk = |name: &str, pass: bool, detail: &str| {
        out.push(format!("  {} {:<28} {}", if pass { "✓" } else { "✗" }, name, detail));
        if !pass { ok = false; }
    };

    let before = all();
    chk("初始可读取", true, &format!("已有 {} 个场景", before.len()));

    // 用两个高编号启动项做夹具，避免撞真实数据
    let a = 901u32;
    let b = 902u32;
    let n1 = save(0, "自检 A", a, 1, 3);
    chk("新建场景", n1.is_ok(), &format!("{:?}", n1));
    let n1 = match n1 { Ok(n) => n, Err(_) => 0 };

    let found = find_by_shortcut(a);
    chk("按启动项反查", found.as_ref().map(|s| s.name.as_str()) == Some("自检 A"),
        &format!("{:?}", found.as_ref().map(|s| s.name.clone())));

    let dup = save(0, "抢同一个启动项", a, 2, 3);
    chk("同启动项被拒", dup.is_err(), &format!("{:?}", dup));

    let reload = {
        // 直接重读磁盘文件，验证序列化往返（load 会整体替换内存）
        load();
        find_by_shortcut(a).map(|s| (s.name, s.group))
    };
    chk("落盘并回读", reload.as_ref().map(|x| x.0.as_str()) == Some("自检 A"),
        &format!("{:?}", reload));

    // ★ 学习区域往返（2026-11-20）：set_region 只改区域，不碰名字/绑定。
    if n1 != 0
    {
        let r = Region { rx: 0.5, ry: 0.7, rw: 0.0, rh: 0.0 };
        let wrote = set_region(n1, "input", Some(r), "测试窗口");
        load();   // 从磁盘重读，验证真的落盘了
        let after = get(n1);
        let ok_region = wrote.is_ok()
            && after.as_ref().and_then(|s| s.input_region) == Some(r)
            && after.as_ref().map(|s| s.name.as_str()) == Some("自检 A")   // 名字没被冲掉
            && after.as_ref().map(|s| s.shortcut_num) == Some(a);           // 绑定没被冲掉
        chk("学习区域写入且不冲掉绑定", ok_region,
            &format!("wrote={:?} region={:?}", wrote,
                after.as_ref().and_then(|s| s.input_region)));

        // ★ 场景常用语 4 组往返（2026-11-20）：set_phrases 落盘 + 激活后按扇区取回
        let ph = ["继续".to_string(), String::new(), "按你的来".to_string(), "好的".to_string()];
        let wrote_ph = set_phrases(n1, ph.clone());
        load();
        set_active(n1);
        let got0 = active_phrase(0);
        let got1 = active_phrase(1);   // 空扇区 → None（回退全局槽）
        let got2 = active_phrase(2);
        let ok_ph = wrote_ph.is_ok()
            && got0.as_deref() == Some("继续")
            && got1.is_none()
            && got2.as_deref() == Some("按你的来")
            && get(n1).map(|s| s.phrases) == Some(ph);
        chk("常用语4组落盘+按扇区取", ok_ph,
            &format!("0={:?} 1={:?} 2={:?}", got0, got1, got2));
        clear_active();
    }

    if n1 != 0 { delete(n1); }
    if let Ok(n2) = save(0, "自检 B", b, 0, 3) {
        delete(n2);
    }
    chk("删除后反查为空", find_by_shortcut(a).is_none() && find_by_shortcut(b).is_none(), "");

    out.push(String::new());
    out.push(if ok { "⇒ 场景存储自检全部通过 ✓" } else { "⇒ ★ 有断言失败" }.to_string());
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_unlinked_shortcut() {
        assert!(validate("WorkBuddy", 0, 0, 3).is_err());
    }

    #[test]
    fn validate_rejects_out_of_range_group() {
        assert!(validate("WorkBuddy", 1, 3, 3).is_err());
        assert!(validate("WorkBuddy", 1, 0, 0).is_err());
        assert!(validate("WorkBuddy", 1, 0, 3).is_ok());
    }

    #[test]
    fn validate_fills_empty_name() {
        assert_eq!(validate("  ", 7, 1, 3).unwrap(), "场景 7");
        assert_eq!(validate(" 微信 ", 7, 1, 3).unwrap(), "微信");
    }

    #[test]
    fn region_roundtrip() {
        let r = Region { rx: 0.1, ry: 0.2, rw: 0.3, rh: 0.4 };
        let v = region_json(Some(r));
        assert_eq!(parse_region(Some(&v)), Some(r));
        assert_eq!(parse_region(Some(&Value::Null)), None);
    }

    #[test]
    fn find_by_shortcut_ignores_zero() {
        // 纯函数边界：0 表示未配置，不能反查到任何场景
        assert!(find_by_shortcut(0).is_none());
    }
}
