//! 场景播放端（2026-11-20）：把学到的锚点变成真实动作。
//!
//! 两件事：
//!   1. `focus_input(num)` —— APP 启动后，等目标窗口出现，按 input_region 点一下输入框。
//!   2. `page_scroll(up)`  —— 上/下键，按当前激活场景的 page_region 在翻页区滚一格。
//!
//! ## 铁律：确定事件，绝不盲点
//! 学习时记下了 `window_title`。播放时**必须**先按这个标题精确匹配到顶层窗口，
//! 才把归一化锚点换算成屏幕坐标去点。找不到 / 超时 / 标题对不上 ⇒ 留痕、不动作。
//! 绝不对着当前前台的任意窗口点 —— 那会误点进别的程序（尤其这台机还有全屏游戏）。

use std::sync::atomic::{AtomicBool, Ordering};

use crate::app_scenes::{self, Region, Scene};

/// 自动聚焦是否正在进行（防止同一场景重复排队）
static FOCUS_BUSY: AtomicBool = AtomicBool::new(false);

/// 把归一化锚点换算成虚拟桌面绝对坐标。
///
/// 用客户区 RECT + ClientToScreen，与学习端 `region_learn` 完全同一套坐标系。
/// rx/ry ∈ [0,1]；rw/rh == 0 表示"点锚点"（学习时只记了一个点）。
#[cfg(target_os = "windows")]
fn anchor_to_screen(scene: &Scene, region: &Region) -> Option<(i32, i32)> {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClientRect, GetWindowTextLengthW,
        GetWindowTextW, IsWindowVisible};

    let want = scene.window_title.trim();
    if want.is_empty() { return None; }

    // 枚举回调要把「期望标题 + 锚点比例 + 命中结果」一起带进去，用上下文结构体指针。
    #[repr(C)]
    struct Ctx<'a> { want: &'a str, rx: f32, ry: f32, hit: Option<(i32, i32)> }
    let mut ctx = Ctx { want, rx: region.rx, ry: region.ry, hit: None };

    unsafe extern "system" fn cb2(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut Ctx<'_>);
        if !IsWindowVisible(hwnd).as_bool() { return BOOL(1); }
        let n = GetWindowTextLengthW(hwnd);
        if n == 0 { return BOOL(1); }
        let mut buf = vec![0u16; n as usize + 1];
        GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n as usize]);
        if !title.eq_ignore_ascii_case(ctx.want) { return BOOL(1); }
        let mut cr = RECT::default();
        if GetClientRect(hwnd, &mut cr).is_err() { return BOOL(1); }
        let mut origin = POINT { x: cr.left, y: cr.top };
        if !ClientToScreen(hwnd, &mut origin).as_bool() { return BOOL(1); }
        let w = cr.right - cr.left;
        let h = cr.bottom - cr.top;
        if w <= 0 || h <= 0 { return BOOL(1); }
        ctx.hit = Some((
            origin.x + (ctx.rx * w as f32) as i32,
            origin.y + (ctx.ry * h as f32) as i32,
        ));
        BOOL(0)
    }

    unsafe {
        let _ = EnumWindows(Some(cb2), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.hit
}

#[cfg(not(target_os = "windows"))]
fn anchor_to_screen(_: &Scene, _: &Region) -> Option<(i32, i32)> { None }

/// 只读诊断（CLI --scene-focus-test）：对场景的两个锚点算出屏幕坐标，不真的点击。
/// 用来验证"窗口标题匹配 + 归一化→绝对坐标"这条链，不干扰用户桌面。
pub fn probe(num: u32) -> String {
    let Some(scene) = app_scenes::get(num) else {
        return format!("场景 #{num} 不存在");
    };
    let mut out = format!("场景「{}」 window_title=\"{}\"\n", scene.name, scene.window_title);
    for (name, region) in [("输入框", scene.input_region), ("翻页区", scene.page_region)] {
        match region {
            Some(r) => match anchor_to_screen(&scene, &r) {
                Some((x, y)) => out.push_str(&format!("  {name} ({:.3},{:.3}) ⇒ 屏幕 ({x},{y})\n", r.rx, r.ry)),
                None => out.push_str(&format!("  {name} ({:.3},{:.3}) ⇒ ✗ 找不到窗口 / 标题不匹配\n", r.rx, r.ry)),
            },
            None => out.push_str(&format!("  {name} 未学习\n")),
        }
    }
    out
}

/// APP 启动后调用：起后台线程等目标窗口出现 → 点输入框。
/// 返回 true = 已排队（或正在聚焦）。
pub fn focus_input(num: u32) -> bool {
    let Some(scene) = app_scenes::get(num) else { return false; };
    let Some(region) = scene.input_region else {
        eprintln!("[场景] 场景 #{num}「{}」未学习输入框，跳过自动聚焦", scene.name);
        return false;
    };
    if scene.window_title.trim().is_empty() {
        eprintln!("[场景] 场景 #{num}「{}」没有窗口标题线索，跳过自动聚焦（防盲点）", scene.name);
        return false;
    }
    if FOCUS_BUSY.swap(true, Ordering::SeqCst) { return false; }

    std::thread::Builder::new()
        .name("scene-focus".into())
        .spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            let mut done = false;
            while std::time::Instant::now() < deadline {
                if let Some((x, y)) = anchor_to_screen(&scene, &region) {
                    // ★ 找到了完全匹配的窗口才点。等 250ms 让它完成布局（输入框就位）。
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    crate::mouse_act::click_at(x, y);
                    eprintln!("[场景] 自动聚焦输入框：窗口「{}」 ({x},{y}) —— 场景 {}",
                        scene.window_title, scene.name);
                    done = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            if !done {
                eprintln!("[场景] 自动聚焦超时（8s 内没找到窗口「{}」）—— 不盲点，保持现状", scene.window_title);
            }
            FOCUS_BUSY.store(false, Ordering::SeqCst);
        })
        .ok();
    true
}

/// 按**当前前台窗口**识别该用哪个场景（2026-11-20）。
///
/// 用途：打开文本轮盘前先确认"用户此刻在哪个程序里"，弹出对应场景的常用语，
///   而不是沿用"上一次按启动键激活的场景"（用户可能早就切走了）。
///
/// 判据（按优先级，任一命中即返回）：
///   1. 场景的 window_title 非空，且当前前台窗口标题**包含**它（学习锚点时记的标题）；
///   2. 场景绑定的启动项，其 path 的文件名（如 WorkBuddy.exe）== 前台进程名。
///
/// 注意：这是"读当前状态来选内容"，不是"猜用户意图来触发动作"——
///   选错内容最坏是弹了另一套短语，用户看清屏再按方向键，无破坏性。
pub fn detect_scene_for_foreground() -> Option<u32> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::Foundation::{CloseHandle, HWND};
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextLengthW,
            GetWindowTextW, GetWindowThreadProcessId};

        let fg: HWND = GetForegroundWindow();
        if fg.0 == 0 { return None; }

        // 前台窗口标题
        let mut title = String::new();
        let n = GetWindowTextLengthW(fg);
        if n > 0 {
            let mut buf = vec![0u16; n as usize + 1];
            GetWindowTextW(fg, &mut buf);
            title = String::from_utf16_lossy(&buf[..n as usize]);
        }

        // 前台进程名（小写，用于不区分大小写比较）
        let mut fg_exe = String::new();
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid != 0 {
            if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 1024];
                let mut size = buf.len() as u32;
                if QueryFullProcessImageNameW(h, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(buf.as_mut_ptr()), &mut size).is_ok() {
                    let full = String::from_utf16_lossy(&buf[..size as usize]);
                    if let Some(name) = full.rsplit(['\\', '/']).next() {
                        fg_exe = name.to_ascii_lowercase();
                    }
                }
                let _ = CloseHandle(h);
            }
        }

        for s in crate::app_scenes::all() {
            // 判据 1：窗口标题包含（用户学过锚点、记下了标题）
            let t = s.window_title.trim();
            if !t.is_empty() && title.to_lowercase().contains(&t.to_lowercase()) {
                return Some(s.num);
            }
            // 判据 2：启动项进程名 == 前台进程名
            if !fg_exe.is_empty() {
                if let Some(sc) = crate::shortcut_store::get(s.shortcut_num) {
                    if let Some(exe) = sc.path.rsplit(['\\', '/']).next() {
                        if !exe.is_empty() && exe.to_ascii_lowercase() == fg_exe {
                            return Some(s.num);
                        }
                    }
                }
            }
        }
    }
    None
}

/// 当前激活场景的**输入框锚点**换算成屏幕坐标（轮盘用来"跟着输入框"）。
/// 没场景 / 没学过输入框 / 找不到窗口 ⇒ None（调用方回退 caret/光标）。
pub fn active_input_anchor() -> Option<(i32, i32)> {
    let s = app_scenes::active_scene()?;
    let r = s.input_region?;
    anchor_to_screen(&s, &r)
}

/// 诊断用：打印当前前台窗口标题、进程名，以及每个场景的匹配线索。
pub fn describe_foreground() -> String {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::Foundation::{CloseHandle, HWND};
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextLengthW,
            GetWindowTextW, GetWindowThreadProcessId};

        let fg: HWND = GetForegroundWindow();
        if fg.0 == 0 { return "前台窗口：<无>".to_string(); }
        let mut title = String::new();
        let n = GetWindowTextLengthW(fg);
        if n > 0 {
            let mut buf = vec![0u16; n as usize + 1];
            GetWindowTextW(fg, &mut buf);
            title = String::from_utf16_lossy(&buf[..n as usize]);
        }
        let mut exe = String::new();
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid != 0 {
            if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 1024];
                let mut size = buf.len() as u32;
                if QueryFullProcessImageNameW(h, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(buf.as_mut_ptr()), &mut size).is_ok() {
                    let full = String::from_utf16_lossy(&buf[..size as usize]);
                    exe = full.rsplit(['\\', '/']).next().unwrap_or("").to_string();
                }
                let _ = CloseHandle(h);
            }
        }
        let mut out = format!("前台窗口 title=\"{title}\" pid={pid} exe=\"{exe}\"\n");
        for s in crate::app_scenes::all() {
            let sc = crate::shortcut_store::get(s.shortcut_num);
            let sc_exe = sc.as_ref().and_then(|c| c.path.rsplit(['\\', '/']).next().map(|x| x.to_string()));
            out.push_str(&format!(
                "  场景#{}「{}」windowTitle=\"{}\" shortcut={} path_exe={:?}\n",
                s.num, s.name, s.window_title, s.shortcut_num, sc_exe
            ));
        }
        out
    }
    #[cfg(not(target_os = "windows"))]
    { String::new() }
}

/// 当前激活场景的翻页：在其 page_region 锚点处滚一格。
/// 窗口找不到 / 没学翻页区 ⇒ 留痕不动作（绝不盲滚到别的窗口）。
pub fn page_scroll(up: bool) {
    let Some(scene) = app_scenes::active_scene() else {
        eprintln!("[场景] 无激活场景，翻页键不动作");
        return;
    };
    let Some(region) = scene.page_region else {
        eprintln!("[场景] 场景「{}」未学习翻页区，翻页键不动作", scene.name);
        return;
    };
    let Some((x, y)) = anchor_to_screen(&scene, &region) else {
        eprintln!("[场景] 找不到窗口「{}」，翻页键不动作（防误滚）", scene.window_title);
        return;
    };
    crate::mouse_act::scroll_at(x, y, up);
    eprintln!("[场景] 翻页{}：窗口「{}」 ({x},{y})", if up { "↑" } else { "↓" }, scene.window_title);
}
