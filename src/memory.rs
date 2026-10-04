/// Process-wide peak resident set size since process start, in bytes.
///
/// A high-water mark: it never goes down, so compare backends in separate processes.
pub fn peak_rss_bytes() -> usize {
    // SAFETY: `rusage` is plain integers, so all-zero is a valid value.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `getrusage` only writes into the struct we pass.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    assert_eq!(
        rc,
        0,
        "getrusage failed: {}",
        std::io::Error::last_os_error()
    );
    let max_rss = usage.ru_maxrss as usize;
    // Apple platforms report bytes; Linux and the BSDs report kilobytes.
    if cfg!(target_vendor = "apple") {
        max_rss
    } else {
        max_rss * 1024
    }
}
