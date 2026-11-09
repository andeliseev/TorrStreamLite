/// Resident memory of this server process, not the browser or macOS app wrapper.
pub fn resident_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut info: libc::mach_task_basic_info = unsafe { std::mem::zeroed() };
        let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
        #[allow(deprecated)]
        let result = unsafe {
            libc::task_info(
                libc::mach_task_self(),
                libc::MACH_TASK_BASIC_INFO,
                &mut info as *mut _ as *mut libc::integer_t,
                &mut count,
            )
        };
        if result == libc::KERN_SUCCESS {
            return Some(info.resident_size);
        }
    }
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in s.lines() {
            if let Some(v) = line.strip_prefix("VmRSS:") {
                return v
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
                    .map(|kb| kb * 1024);
            }
        }
    }
    None
}

/// The memory figure the app shows: on macOS the physical footprint, which Activity Monitor
/// shows too (it also counts compressed pages, and not shared libraries); elsewhere the
/// resident size.
pub fn memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::proc_pid_rusage(
                libc::getpid(),
                libc::RUSAGE_INFO_V2,
                &mut info as *mut _ as *mut libc::rusage_info_t,
            )
        };
        if result == 0 {
            return Some(info.ri_phys_footprint);
        }
    }
    resident_bytes()
}

#[cfg(test)]
mod tests {
    #[test]
    fn memory_figures_cover_a_touched_allocation() {
        // Other tests may free a 16 MiB cache meanwhile.
        let before = super::memory_bytes().unwrap();
        let block = vec![1u8; 64 << 20];
        let after = super::memory_bytes().unwrap();
        assert!(after >= before + (40 << 20), "{before} -> {after}");
        assert!(super::resident_bytes().unwrap() >= 40 << 20);
        drop(block);
    }
}
