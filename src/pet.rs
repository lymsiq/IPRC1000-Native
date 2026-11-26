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
/// 把某个程序的托盘图标**收进溢出区 / 放回任务栏**（2026-11-26）。
///
/// ## 为什么只能"收进溢出"而不是"彻底隐藏"
/// Windows 11 通知区只有两种状态：常显、溢出（点 `^` 才看到）。
/// 没有"程序不显示图标"的系统开关——图标是程序自己 `Shell_NotifyIcon` 加的，
/// 只能由它自己删，而 KuroCat 没有这个选项（它的配置里一堆开关，就是没有托盘项）。
/// 所以用系统的合法机制：改 `HKCU\Control Panel\NotifyIconSettings\*\IsPromoted`。
///
/// `exe_contains` 按 `ExecutablePath` 做**包含匹配**：用户可能从微信/资源管理器
/// 开过多个副本（真机上就有一个 `KuroCat-Windows-x64 (1).exe`），全部处理。
pub fn set_tray_promoted(exe_contains: &str, promoted: bool) -> Result<usize, String> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Registry::{
            RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
            HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD,
        };

        fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() }

        let mut root = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            windows::core::w!("Control Panel\\NotifyIconSettings"),
            0,
            KEY_READ | KEY_WRITE,
            &mut root,
        ) != ERROR_SUCCESS
        {
            return Err("打不开 NotifyIconSettings".into());
        }

        let want = exe_contains.to_ascii_lowercase();
        let mut changed = 0usize;
        let mut idx = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut nlen = name.len() as u32;
            if RegEnumKeyExW(
                root, idx, windows::core::PWSTR(name.as_mut_ptr()), &mut nlen,
                None, windows::core::PWSTR::null(), None, None,
            ) != ERROR_SUCCESS
            {
                break;
            }
            idx += 1;
            let key_w = wide(&String::from_utf16_lossy(&name[..nlen as usize]));

            let mut child = HKEY::default();
            if RegOpenKeyExW(root, PCWSTR(key_w.as_ptr()), 0, KEY_READ | KEY_WRITE, &mut child)
                != ERROR_SUCCESS
            {
                continue;
            }
            let pname = windows::core::w!("ExecutablePath");
            let mut ty = windows::Win32::System::Registry::REG_VALUE_TYPE::default();
            let mut sz = 0u32;
            let mut path_ok = false;
            if RegQueryValueExW(child, pname, None, Some(&mut ty), None, Some(&mut sz)) == ERROR_SUCCESS
                && sz > 0
            {
                let mut buf = vec![0u8; sz as usize];
                if RegQueryValueExW(child, pname, None, Some(&mut ty), Some(buf.as_mut_ptr()), Some(&mut sz))
                    == ERROR_SUCCESS
                {
                    let s: Vec<u16> = buf
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .take_while(|&c| c != 0)
                        .collect();
                    path_ok = String::from_utf16_lossy(&s).to_ascii_lowercase().contains(&want);
                }
            }
            if path_ok {
                let bytes = (if promoted { 1u32 } else { 0u32 }).to_le_bytes();
                if RegSetValueExW(child, windows::core::w!("IsPromoted"), 0, REG_DWORD, Some(&bytes))
                    == ERROR_SUCCESS
                {
                    changed += 1;
                }
            }
            let _ = RegCloseKey(child);
        }
        let _ = RegCloseKey(root);
        Ok(changed)
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = (exe_contains, promoted); Ok(0) }
}

/// 桌宠的托盘图标收进溢出区（任务栏只留本系统一个图标）
pub fn hide_tray_icon() -> Result<usize, String> { set_tray_promoted("kurocat", false) }
/// 桌宠的托盘图标放回任务栏
pub fn show_tray_icon() -> Result<usize, String> { set_tray_promoted("kurocat", true) }

/// 开关状态持久化：`<程序目录>/data/pet.json`（只存一个 enabled）。
/// 放在 data/ 下，与 native.json 同目录，卸载时一并清掉。
fn cfg_path() -> PathBuf {
    crate::paths::data_dir().join("pet.json")
}

/// 上次是否开着桌宠（程序重启后据此自动拉起）
///
/// ⚠ 要容忍 BOM：用户用记事本 / PowerShell 5.1 另存会带 `EF BB BF`，
///   serde_json 直接报错 ⇒ 表现为"明明写着 true 却不自动启动"（实测踩过）。
pub fn load_enabled() -> bool {
    let raw = match std::fs::read(cfg_path()) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let txt = String::from_utf8_lossy(&raw);
    let txt = txt.trim_start_matches('\u{feff}').trim();
    serde_json::from_str::<serde_json::Value>(txt)
        .ok()
        .and_then(|v| v.get("enabled").and_then(|x| x.as_bool()))
        .unwrap_or(false)
}

pub fn set_enabled(v: bool) {
    let p = cfg_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let body = serde_json::json!({ "enabled": v }).to_string();
    if let Err(e) = std::fs::write(&p, body) {
        eprintln!("[桌宠] 写配置失败：{e}");
    }
}

/// 把桌宠绑定到本进程的 Job Object（2026-11-26）
///
/// 为什么需要：托盘「退出」我们本来就会 stop()，但**崩溃 / 被强杀 / 蓝屏前**走不到那行，
/// 猫会孤零零留在桌面上 —— 那就不像"一个系统"了。
/// Job 设 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`：本进程句柄一关（含异常退出），
/// 内核自动杀掉同 Job 的子进程。这是系统级保证，比"退出时记得清理"可靠。
///
/// 只对**我们自己 spawn 的**那只生效：用户从微信开的副本不在这个 Job 里，
/// 那本来就是他自己的进程，我们不该替他做主。
#[cfg(target_os = "windows")]
fn bind_to_job(pid: u32) {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    static JOB: OnceLock<isize> = OnceLock::new();
    unsafe {
        let job = JOB.get_or_init(|| {
            let Ok(h) = CreateJobObjectW(None, None) else { return 0 };
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let _ = SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            h.0 as isize
        });
        if *job == 0 { return; }
        let job_h = windows::Win32::Foundation::HANDLE(*job);
        if let Ok(p) = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) {
            if AssignProcessToJobObject(job_h, p).is_err() {
                eprintln!("[桌宠] 绑定 Job 失败（退出时需手动关它）");
            }
            let _ = CloseHandle(p);
        }
    }
}

/// 启动桌宠。返回 `Ok(true)` = 现在在跑（原本就跑，或刚起来）。
pub fn start() -> Result<bool, String> {
    if is_running() {
        return Ok(true);
    }
    let path = exe_path().ok_or_else(|| {
        format!("找不到 {EXE_NAME}（应位于 assets/pet/ 下；重装本程序可恢复）")
    })?;
    spawn_pet(&path, false)
}

/// 常驻模式专用：起猫并**绑进本进程 Job**（本进程异常退出时系统一并收掉它）。
/// 与 `start()` 的区别只有这一点 —— CLI 的 `--pet start` 必须用 `start()`，
/// 否则我们一 exit(0) 就把它带走了（实测会"起不来"）。
pub fn start_bound() -> Result<bool, String> {
    if is_running() {
        return Ok(true);
    }
    let path = exe_path().ok_or_else(|| {
        format!("找不到 {EXE_NAME}（应位于 assets/pet/ 下；重装本程序可恢复）")
    })?;
    spawn_pet(&path, true)
}

fn spawn_pet(path: &std::path::Path, bind_job: bool) -> Result<bool, String> {
    let child = std::process::Command::new(path)
        .current_dir(path.parent().unwrap_or(std::path::Path::new(".")))
        .spawn()
        .map_err(|e| format!("启动失败：{e}"))?;
    // ★ 绑进本进程的 Job：我们异常退出时系统会一并收掉它（见 bind_to_job）
    if bind_job {
        #[cfg(target_os = "windows")]
        bind_to_job(child.id());
    }
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

