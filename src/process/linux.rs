use std::collections::HashSet;
use std::fs;

pub fn snapshot_running_processes() -> HashSet<String> {
    let entries = match fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(_) => return HashSet::new(),
    };

    let mut running = HashSet::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid = name.to_string_lossy();
        if !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let proc_dir = entry.path();

        // `/proc/<pid>/comm` is truncated to 15 characters (TASK_COMM_LEN).
        // Still useful for short names and as a fast path.
        if let Ok(raw) = fs::read_to_string(proc_dir.join("comm")) {
            let normalized = raw.trim().to_ascii_lowercase();
            if !normalized.is_empty() {
                running.insert(normalized);
            }
        }

        // Prefer argv0 from cmdline so long binaries like google-chrome-stable
        // and microsoft-edge-stable match processRunning() candidates.
        if let Ok(raw) = fs::read(proc_dir.join("cmdline")) {
            if let Some(argv0) = raw.split(|&b| b == 0).find(|part| !part.is_empty()) {
                if let Ok(path) = std::str::from_utf8(argv0) {
                    let normalized = path.trim().to_ascii_lowercase();
                    if !normalized.is_empty() {
                        running.insert(normalized);
                    }
                }
            }
        }
    }
    running
}
