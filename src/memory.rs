pub fn peak_rss_bytes() -> usize {
    let max_rss = rusage().ru_maxrss as usize;
    if cfg!(target_vendor = "apple") {
        max_rss
    } else {
        max_rss * 1024
    }
}

/// Minor (soft) page faults so far: mostly first touches of freshly mapped
/// pages. Sample before and after a step to count the faults it caused.
pub fn minor_faults() -> u64 {
    rusage().ru_minflt as u64
}

/// Resident set size right now, in bytes (unlike [`peak_rss_bytes`], it falls
/// when memory is returned to the OS).
#[cfg(target_vendor = "apple")]
pub fn current_rss_bytes() -> usize {
    unsafe extern "C" {
        static mach_task_self_: libc::mach_port_t;
    }
    let mut info: libc::mach_task_basic_info = unsafe { std::mem::zeroed() };
    let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
    let rc = unsafe {
        libc::task_info(
            mach_task_self_,
            libc::MACH_TASK_BASIC_INFO,
            (&raw mut info).cast(),
            &mut count,
        )
    };
    assert_eq!(rc, libc::KERN_SUCCESS, "task_info failed: {rc}");
    info.resident_size as usize
}

/// Resident set size right now, in bytes (unlike [`peak_rss_bytes`], it falls
/// when memory is returned to the OS).
#[cfg(not(target_vendor = "apple"))]
pub fn current_rss_bytes() -> usize {
    use std::io::Read;

    // "size resident shared text lib data dt", in pages.
    let mut buf = [0u8; 128];
    let len = std::fs::File::open("/proc/self/statm")
        .and_then(|mut file| file.read(&mut buf))
        .expect("read /proc/self/statm");
    let resident: usize = std::str::from_utf8(&buf[..len])
        .ok()
        .and_then(|statm| statm.split_whitespace().nth(1))
        .and_then(|pages| pages.parse().ok())
        .expect("parse /proc/self/statm");
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(page > 0, "sysconf(_SC_PAGESIZE) failed");
    resident * page as usize
}

fn rusage() -> libc::rusage {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    assert_eq!(
        rc,
        0,
        "getrusage failed: {}",
        std::io::Error::last_os_error()
    );
    usage
}
