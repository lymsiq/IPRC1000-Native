//! 桌宠（KuroCat）托管（2026-11-26）
//!
//! ## 为什么不把桌宠嵌进本进程
//! KuroCat 是独立的 C++17 单文件程序（MIT），**自带 Raw Input 按键捕获**。
//! 我们注入按键走 WinUHid 虚拟 HID（驱动层，≡ 真实键盘，**不带 LLKHF_INJECTED**），
//! 所以遥控器按下的键、语音键（右 Alt 按住）它**直接就能看到** —— 不需要任何桥接代码。
//! 这里只负责"起停"这一个动作。
//!
//! ## 单实例（真机踩过）
//! KuroCat 自带互斥体：已有实例时新进程会**立即退出、退出码 0**（看起来像"启动失败"）。
//! 所以"是否在运行"必须**按进程名扫描**，不能只记自己 spawn 的 PID——
//! 用户完全可能从微信/资源管理器另外开一个，路径我们根本不知道。
//!
//! ## 触发能力的边界（要如实告知用户）
//! - ✅ 能触发：注入成键盘的键（方向键/回车/音量/文本与宏里的字符）、语音键（右 Alt）
//! - ❌ 不能触发：纯鼠标动作、滚轮 —— KuroCat 只抓键盘
//! - ⚠ 注入了但看不到：WinUHid 不可用、退化到 SendInput 时，键可能带 INJECTED 标记

use std::path::PathBuf;

/// 我们放进包里的可执行名
pub const EXE_NAME: &str = "KuroCat.exe";
/// 进程名匹配前缀（用户从别处开的副本可能叫 `KuroCat-Windows-x64 (1)`）
const PROC_PREFIX: &str = "kurocat";

/// 找可执行文件：exe 同级的 `assets/pet/KuroCat.exe`（打包后的位置），
/// 其次开发树的 `iprc1000_native/assets/pet/KuroCat.exe`。
pub fn exe_path() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join("assets").join("pet").join(EXE_NAME));
            // 开发态：target/release/ 往上两级是仓库根（iprc1000_native/）
            cands.push(dir.join("..").join("..").join("assets").join("pet").join(EXE_NAME));
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

/// 是否已有 KuroCat 在跑（任意路径、任意副本名）。
pub fn is_running() -> bool {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return false };
        let mut pe = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let name = String::from_utf16_lossy(
                    &pe.szExeFile[..pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len())],
                );
                if name.to_ascii_lowercase().starts_with(PROC_PREFIX) {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
    #[cfg(not(target_os = "windows"))]
    { false }
}

/// 启动桌宠。返回 `Ok(true)` = 现在在跑（原本就跑，或刚起来）。
pub fn start() -> Result<bool, String> {
    if is_running() {
        return Ok(true);
    }
    let path = exe_path().ok_or_else(|| {
        format!("找不到 {EXE_NAME}（应位于 assets/pet/ 下；重装本程序可恢复）")
    })?;
    std::process::Command::new(&path)
        .current_dir(path.parent().unwrap_or(std::path::Path::new(".")))
        .spawn()
        .map_err(|e| format!("启动失败：{e}"))?;
    // 给它一点时间建窗口，再回报状态（单实例互斥时这里会重新读到已有实例）
    std::thread::sleep(std::time::Duration::from_millis(600));
    Ok(is_running())
}

/// 关闭桌宠（结束所有 KuroCat* 进程）。返回结束的数量。
pub fn stop() -> Result<u32, String> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Err("进程快照失败".into());
        };
        let mut pe = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut n = 0u32;
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let name = String::from_utf16_lossy(
                    &pe.szExeFile[..pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len())],
                );
                if name.to_ascii_lowercase().starts_with(PROC_PREFIX) {
                    if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pe.th32ProcessID) {
                        if TerminateProcess(h, 0).is_ok() {
                            n += 1;
                        }
                        let _ = CloseHandle(h);
                    }
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        Ok(n)
    }
    #[cfg(not(target_os = "windows"))]
    { Ok(0) }
}

