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
        let comm = entry.path().join("comm");
        if let Ok(raw) = fs::read_to_string(comm) {
            let normalized = raw.trim().to_ascii_lowercase();
            if !normalized.is_empty() {
                running.insert(normalized);
            }
        }
    }
    running
}
