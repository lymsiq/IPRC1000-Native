//! 手动点击学习区域（2026-11-20）
//!
//! ## 交互
//! 用户在场景页点「学习输入框」→ 3 秒倒计时（切到 WorkBuddy 并点好位置）
//! → 程序在后台轮询到一次左键点击 → 取该点相对前台窗口的归一化坐标 → 写回场景。
//!
//! ## 为什么用轮询而不是鼠标钩子
//! · 只需"一次点击的坐标"，轮询 40ms 足够，精度远超锚点所需；
//! · 不装 WH_MOUSE_LL、不吞事件、不引入消息循环线程 —— 项目里钩子只留给录制/幽灵键，
//!   学习不该碰输入链路。
//!
//! ## 为什么是归一化坐标
//! 实测 WorkBuddy 窗口在扩展桌面 x=2552 起、宽 2560，绝对坐标换个显示器全废。
//! 存 (rx, ry) ∈ [0,1]（相对前台窗口客户区），播放时按窗口当前 RECT 还原。
//!
//! ## 判据（防误学）
//! · 忽略点在我们自己窗口上的点击（按进程 ID 排除）；
//! · 只认有标题的顶层窗口（排除桌面/任务栏）；
//! · 捕获后立刻停，一次学习只记一个点；取消随时可用。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::app_scenes::Region;

/// 学习状态（给 UI 显示）。
#[derive(Clone, Debug, PartialEq)]
pub enum LearnState {
    Idle,
    /// 倒计时中，剩余秒数
    Countdown(u64),
    /// 监听点击中，已等待秒数
    Waiting(u64),
    /// 刚捕获到一个点（坐标 + 窗口标题），等待 UI 落盘后回 Idle
    Captured { rx: f32, ry: f32, window: String },
}

static STATE: Mutex<LearnState> = Mutex::new(LearnState::Idle);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
/// 要写入的场景号与区域类型："input" / "page"
static TARGET_NUM: AtomicU32 = AtomicU32::new(0);
static TARGET_KIND: Mutex<String> = Mutex::new(String::new());
static START_MS: AtomicU64 = AtomicU64::new(0);

pub fn state() -> LearnState {
    STATE.lock().map(|s| s.clone()).unwrap_or(LearnState::Idle)
}
pub fn is_active() -> bool { ACTIVE.load(Ordering::Relaxed) }

fn set_state(s: LearnState) {
    if let Ok(mut g) = STATE.lock() { *g = s; }
}

/// 启动一次学习。返回 false = 已在学中。
pub fn start(num: u32, kind: &str) -> bool {
    if ACTIVE.swap(true, Ordering::SeqCst) { return false; }
    CANCEL.store(false, Ordering::SeqCst);
    TARGET_NUM.store(num, Ordering::SeqCst);
    if let Ok(mut g) = TARGET_KIND.lock() { *g = kind.to_string(); }
    START_MS.store(now_ms(), Ordering::Relaxed);
    set_state(LearnState::Countdown(3));

    std::thread::Builder::new()
        .name("region-learn".into())
        .spawn(|| {
            let _ = learn_loop();
            ACTIVE.store(false, Ordering::SeqCst);
            // 留 1.2 秒让用户看到"已捕获"提示，再回 Idle
            std::thread::sleep(Duration::from_millis(1200));
            if !CANCEL.load(Ordering::Relaxed) { set_state(LearnState::Idle); }
        })
        .ok();
    true
}

/// 取消学习。
pub fn cancel() {
    if !ACTIVE.load(Ordering::Relaxed) { return; }
    CANCEL.store(true, Ordering::SeqCst);
    set_state(LearnState::Idle);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 把屏幕点换算成相对窗口客户区的归一化坐标（纯函数，可单测）。
///
/// 返回值 None = 点不在窗口内（误点，忽略）。
/// 用客户区 RECT（left/top + width/height），与 Windows 的 GetClientRect + ClientToScreen 对齐。
pub fn normalize_point(px: i32, py: i32, left: i32, top: i32, width: i32, height: i32) -> Option<(f32, f32)> {
    if width <= 0 || height <= 0 { return None; }
    let rx = (px - left) as f32 / width as f32;
    let ry = (py - top) as f32 / height as f32;
    if !(0.0..=1.0).contains(&rx) || !(0.0..=1.0).contains(&ry) { return None; }
    Some((rx, ry))
}

#[cfg(target_os = "windows")]
fn learn_loop() -> Result<(), ()> {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    let t0 = Instant::now();
    // ① 倒计时 3 秒：给用户时间切到目标窗口、把鼠标移到输入框
    while t0.elapsed() < Duration::from_secs(3) {
        if CANCEL.load(Ordering::Relaxed) { return Err(()); }
        let left = 3u64.saturating_sub(t0.elapsed().as_secs());
        set_state(LearnState::Countdown(left));
        std::thread::sleep(Duration::from_millis(100));
    }

    set_state(LearnState::Waiting(0));
    let watch = Instant::now();
    let self_pid = std::process::id();

    // ② 监听左键按下（边沿触发：上次没按、这次按下）
    let mut prev_down = unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 };
    loop {
        if CANCEL.load(Ordering::Relaxed) { return Err(()); }
        std::thread::sleep(Duration::from_millis(40));
        set_state(LearnState::Waiting(watch.elapsed().as_secs()));
        // 60 秒超时：不让人一直挂着学习态
        if watch.elapsed() > Duration::from_secs(60) { return Err(()); }

        let down = unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 };
        if !down || prev_down { prev_down = down; continue; }
        prev_down = down;

        unsafe {
            let fg = GetForegroundWindow();
            if fg.0 == 0 { continue; }
            // 忽略点在我们自己窗口上的点击
            let mut fg_pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut fg_pid));
            if fg_pid == self_pid { continue; }

            // 前台窗口必须有标题（排除桌面/任务栏）
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(fg, &mut buf);
            if n == 0 { continue; }
            let title = String::from_utf16_lossy(&buf[..n as usize]);

            let mut p = POINT::default();
            if GetCursorPos(&mut p).is_err() { continue; }

            // 客户区 RECT → 屏幕坐标
            let mut cr = RECT::default();
            if GetClientRect(fg, &mut cr).is_err() { continue; }
            let mut origin = POINT { x: cr.left, y: cr.top };
            if !ClientToScreen(fg, &mut origin).as_bool() { continue; }
            let w = cr.right - cr.left;
            let h = cr.bottom - cr.top;

            let Some((rx, ry)) = normalize_point(p.x, p.y, origin.x, origin.y, w, h) else { continue };

            let num = TARGET_NUM.load(Ordering::SeqCst);
            let kind = TARGET_KIND.lock().map(|g| g.clone()).unwrap_or_default();
            match crate::app_scenes::set_region(num, &kind, Some(Region { rx, ry, rw: 0.0, rh: 0.0 }), &title) {
                Ok(()) => {
                    set_state(LearnState::Captured { rx, ry, window: title.clone() });
                    eprintln!("[学习] 场景 #{num} {kind} 锚点=({rx:.3},{ry:.3}) 窗口=\"{title}\"");
                }
                Err(e) => eprintln!("[学习] 写回失败：{e}"),
            }
            return Ok(());
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn learn_loop() -> Result<(), ()> { Err(()) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_center() {
        assert_eq!(normalize_point(150, 250, 100, 200, 100, 100), Some((0.5, 0.5)));
    }
    #[test]
    fn normalize_edges() {
        assert_eq!(normalize_point(100, 200, 100, 200, 100, 100), Some((0.0, 0.0)));
        assert_eq!(normalize_point(200, 300, 100, 200, 100, 100), Some((1.0, 1.0)));
    }
    #[test]
    fn normalize_outside_rejected() {
        assert!(normalize_point(99, 250, 100, 200, 100, 100).is_none());
        assert!(normalize_point(150, 301, 100, 200, 100, 100).is_none());
    }
    #[test]
    fn normalize_bad_size_rejected() {
        assert!(normalize_point(150, 250, 100, 200, 0, 100).is_none());
        assert!(normalize_point(150, 250, 100, 200, 100, -5).is_none());
    }
}
