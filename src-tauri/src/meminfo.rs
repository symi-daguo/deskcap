//! 进程内存自报。
//!
//! 为什么需要：DeskCap 是 Tauri 应用，界面跑在系统 WebKit 里。外部工具看到的
//! 数字口径混乱 —— `ps` 的 RSS 只算独占页（~86MB），`vmmap` 的 RESIDENT 含共享
//! 系统库（~736MB），虚拟地址空间是几百 GB 的预留（不占物理）。维护者被这个
//! 差异问过好几次，所以让进程自己报一次，口径固定、可对照。
//!
//! macOS 用 `proc_pid_rusage` 的 `ri_phys_footprint` —— 这正是活动监视器
//! 「内存」列显示的值，也是内存压力统计用的口径。

#[cfg(target_os = "macos")]
fn rusage_v4() -> Option<libc::rusage_info_v4> {
    unsafe {
        let mut info: libc::rusage_info_v4 = std::mem::zeroed();
        // libc 把第三参数声明成 `*mut rusage_info_t`（即 `*mut *mut c_void`），
        // 但内核是按「指向结构体的指针」写入的，所以这里把结构体地址强转过去。
        let rc = libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V4,
            &mut info as *mut libc::rusage_info_v4 as *mut libc::rusage_info_t,
        );
        if rc == 0 {
            Some(info)
        } else {
            None
        }
    }
}

/// 当前进程真实物理占用（MB）。取不到时返回 `None`。
///
/// macOS：`ri_phys_footprint`，与活动监视器一致。
/// Windows / Linux：暂不提供（返回 `None`，不猜、不编数字）。
#[cfg(target_os = "macos")]
pub fn phys_footprint_mb() -> Option<u64> {
    rusage_v4().map(|i| (i.ri_phys_footprint as u64) / 1_048_576)
}

/// 进程生命周期内的物理占用峰值（MB）。
#[cfg(target_os = "macos")]
pub fn phys_footprint_peak_mb() -> Option<u64> {
    rusage_v4().map(|i| (i.ri_lifetime_max_phys_footprint as u64) / 1_048_576)
}

#[cfg(not(target_os = "macos"))]
pub fn phys_footprint_mb() -> Option<u64> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn phys_footprint_peak_mb() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprint_is_plausible() {
        let Some(cur) = phys_footprint_mb() else {
            // 非 macOS 平台直接跳过
            return;
        };
        // 测试进程只加载标准库，几十 MB 量级；给宽松上限防误报
        assert!(cur > 0, "footprint 应为正数");
        assert!(cur < 4096, "测试进程不该超过 4GB，实际 {cur}MB");

        if let Some(peak) = phys_footprint_peak_mb() {
            assert!(peak >= cur, "峰值 {peak}MB 应不小于当前 {cur}MB");
        }
    }
}
