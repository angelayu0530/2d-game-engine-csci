pub fn peak_rss_bytes() -> usize {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    assert_eq!(
        rc,
        0,
        "getrusage failed: {}",
        std::io::Error::last_os_error()
    );
    let max_rss = usage.ru_maxrss as usize;
    if cfg!(target_vendor = "apple") {
        max_rss
    } else {
        max_rss * 1024
    }
}
