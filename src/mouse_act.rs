//! 鼠标动作注入（2026-10-31）
//!
//! 遥控器上任意一个键都可以被绑成"鼠标动作"，用负数哨兵表示（与 C# `KeyAct` 一致）：
//!   -1 左键 · -2 右键 · -3 滚轮上 · -4 滚轮下 · -5 切换配置组 · -6 截图点击
//!
//! 为什么不能走键盘注入：这些不是 HID 键盘 usage，塞进键盘报告只会变成乱码键。
//! 鼠标必须用 `SendInput`（MOUSEEVENTF_*），这也是 C# 版当年的做法。
//!
//! ⚠ 与键盘注入的关键差别：`SendInput` 发的鼠标事件**带 LLMHF_INJECTED 标记**，
//! 而 WinUHid 键盘不带。所以如果以后加"忽略自己产生的事件"，鼠标和键盘要分开判。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT,
};

pub const ACT_LEFT: i32 = -1;
pub const ACT_RIGHT: i32 = -2;
pub const ACT_WHEEL_UP: i32 = -3;
pub const ACT_WHEEL_DOWN: i32 = -4;
pub const ACT_SWITCH_GROUP: i32 = -5;
pub const ACT_CLICK_ICON: i32 = -6;
/// 文本轮盘（2026-11-10）：按下这个键，屏幕上弹出 4 方向轮盘，
/// 遥控器方向键选一条文本，OK 确认后自动输入。运行时 UI 见 `ui_wheel.rs`。
pub const ACT_TEXT_WHEEL: i32 = -3000;

/// 动作的中文名（界面与日志共用）
pub fn action_name(act: i32) -> &'static str {
    match act {
        ACT_LEFT => "鼠标左键",
        ACT_RIGHT => "鼠标右键",
        ACT_WHEEL_UP => "滚轮上",
        ACT_WHEEL_DOWN => "滚轮下",
        ACT_SWITCH_GROUP => "切换配置组",
        ACT_CLICK_ICON => "截图点击",
        ACT_TEXT_WHEEL => "文本轮盘",
        ACT_PAGE_UP => "场景翻页↑",
        ACT_PAGE_DOWN => "场景翻页↓",
        _ => "未知动作",
    }
}

/// 是不是"按住型"动作（按下/松开各有意义）。
/// 滚轮和切组是**一次性**的，只在按下那一刻触发，松开什么都不做。
#[allow(dead_code)] // 未接：按住型鼠标动作的超时释放
pub fn is_hold_action(act: i32) -> bool {
    matches!(act, ACT_LEFT | ACT_RIGHT)
}

fn send_mouse(flags: u32, data: i32) {
    let inp = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data as u32,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS(flags),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(&[inp], std::mem::size_of::<INPUT>() as i32);
    }
}

/// 执行一个鼠标动作。`down` = 遥控器上是按下还是松开。
///
/// 返回 true 表示"这个动作被处理了"，调用方就不要把它当键盘键处理。
pub fn fire(act: i32, down: bool) -> bool {
    match act {
        // 轮盘是运行时 UI，真正的打开动作在 backend 的按键分派里做；
        // 这里只负责"认领"，避免这个键再被注入成键盘键。
        ACT_TEXT_WHEEL => true,
        ACT_LEFT => {
            send_mouse(
                if down { MOUSEEVENTF_LEFTDOWN.0 } else { MOUSEEVENTF_LEFTUP.0 },
                0,
            );
            true
        }
        ACT_RIGHT => {
            send_mouse(
                if down { MOUSEEVENTF_RIGHTDOWN.0 } else { MOUSEEVENTF_RIGHTUP.0 },
                0,
            );
            true
        }
        // 滚轮只在按下时滚一格（WHEEL_DELTA = 120）
        ACT_WHEEL_UP => {
            if down {
                send_mouse(MOUSEEVENTF_WHEEL.0, 120);
            }
            true
        }
        ACT_WHEEL_DOWN => {
            if down {
                send_mouse(MOUSEEVENTF_WHEEL.0, -120);
            }
            true
        }
        // 切组由 backend 处理（它要改运行时状态），这里只报告"认领了"
        ACT_SWITCH_GROUP => true,
        // 截图点击需要模板匹配，原生版还没有 —— 认领但不动作，避免漏成键盘键
        ACT_CLICK_ICON => true,
        // 场景翻页：由 backend 查当前激活场景的 page_region 后调 page_scroll，
        // 这里只认领，防止漏成键盘键。
        ACT_PAGE_UP | ACT_PAGE_DOWN => true,
        _ => false,
    }
}

// ── 场景自动化原语（2026-11-20）──

/// 场景翻页动作值：上/下。与鼠标动作同段（-1..-6）延续，取 -7/-8。
/// 具体翻哪、在哪翻，由 backend 查当前激活场景的 page_region 决定。
pub const ACT_PAGE_UP: i32 = -7;
pub const ACT_PAGE_DOWN: i32 = -8;

/// 绝对坐标移动（虚拟桌面）。
///
/// ★ 必须带 MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK：
///   这台机器是 2560x1440 扩展桌面，缺 VIRTUALDESK 会把坐标夹到主屏 ⇒ 点到错误位置
///   （C# 版踩过同一个坑，见 MouseInjector.MoveAbsolute）。
fn move_abs(x: i32, y: i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
    unsafe {
        // Windows 约定：ABSOLUTE 坐标归一化到 0..65535，映射到整个虚拟桌面。
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);
        let nx = ((x - vx) as i64 * 65535 / vw as i64).clamp(0, 65535) as i32;
        let ny = ((y - vy) as i64 * 65535 / vh as i64).clamp(0, 65535) as i32;
        let inp = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: nx,
                    dy: ny,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let _ = SendInput(&[inp], std::mem::size_of::<INPUT>() as i32);
    }
}

/// 在虚拟桌面绝对坐标点一下（用于自动聚焦输入框）。
pub fn click_at(x: i32, y: i32) {
    move_abs(x, y);
    std::thread::sleep(std::time::Duration::from_millis(30)); // 让移动先落地，否则点击落在旧位置
    send_mouse(MOUSEEVENTF_LEFTDOWN.0, 0);
    std::thread::sleep(std::time::Duration::from_millis(40));
    send_mouse(MOUSEEVENTF_LEFTUP.0, 0);
}

/// 在虚拟桌面绝对坐标处滚一格（up=true 向上）。用于场景翻页。
pub fn scroll_at(x: i32, y: i32, up: bool) {
    move_abs(x, y);
    std::thread::sleep(std::time::Duration::from_millis(30));
    send_mouse(MOUSEEVENTF_WHEEL.0, if up { 120 } else { -120 });
}
