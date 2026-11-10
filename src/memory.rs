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
    if let Some(resident) = status_bytes("VmRSS:") {
        return Some(resident);
    }
    None
}

/// A memory figure from `/proc/self/status`, where the kernel gives it in kB.
#[cfg(target_os = "linux")]
fn status_bytes(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kib: u64 = status
        .lines()
        .find_map(|line| line.strip_prefix(field))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kib * 1024)
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

/// The most memory this server has had resident at once, in bytes.
pub fn peak_resident_bytes() -> Option<u64> {
    // The kernel's own high-water mark. On Linux, getrusage's peak also counts the program
    // that started this one, as large as it was before it became the server.
    #[cfg(target_os = "linux")]
    if let Some(peak) = status_bytes("VmHWM:") {
        return Some(peak);
    }
    // macOS counts it in bytes.
    #[cfg(target_os = "macos")]
    if let Some(usage) = rusage() {
        return Some(usage.ru_maxrss as u64);
    }
    None
}

/// The processor time this server has used since it started, in seconds. For measuring a
/// run from outside, on the router above all: the time used between two readings, over
/// the time between them, is the load.
pub fn cpu_seconds() -> Option<f64> {
    let usage = rusage()?;
    let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1e6;
    Some(seconds(usage.ru_utime) + seconds(usage.ru_stime))
}

fn rusage() -> Option<libc::rusage> {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    (unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0).then_some(usage)
}

#[cfg(test)]
mod tests {
    #[test]
    fn processor_time_and_peak_memory_are_counted() {
        let before = super::cpu_seconds().unwrap();
        let block = vec![1u8; 48 << 20];
        // Work until the clock has moved on. Other tests' threads move it too.
        let mut sum = 0u64;
        loop {
            sum = block
                .iter()
                .fold(sum, |sum, &byte| sum.wrapping_add(byte as u64));
            if super::cpu_seconds().unwrap() >= before + 0.05 {
                break;
            }
        }
        assert!(sum >= 48 << 20);
        // The block was resident when it was summed. The upper bound catches a wrong unit:
        // one system counts the peak in bytes, the other in kB.
        let peak = super::peak_resident_bytes().unwrap();
        assert!((40 << 20..4 << 30).contains(&peak), "peak {peak}");
        assert!(peak >= super::resident_bytes().unwrap() / 2);
    }

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
