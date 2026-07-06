use crate::browser::registry;
use std::cell::RefCell;
use std::collections::HashSet;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

thread_local! {
    static ROUTE_SNAPSHOT: RefCell<Option<HashSet<String>>> = const { RefCell::new(None) };
}

/// Clears any process snapshot cached during a single route evaluation.
pub struct RouteProcessGuard;

impl Default for RouteProcessGuard {
    fn default() -> Self {
        Self
    }
}

impl RouteProcessGuard {
    pub fn new() -> Self {
        Self
    }
}

impl Drop for RouteProcessGuard {
    fn drop(&mut self) {
        ROUTE_SNAPSHOT.with(|cell| *cell.borrow_mut() = None);
    }
}

#[cfg(test)]
pub fn replace_snapshot_for_tests(names: HashSet<String>) {
    ROUTE_SNAPSHOT.with(|cell| *cell.borrow_mut() = Some(names));
}

pub fn is_running(query: &str) -> bool {
    let browser_id = registry::normalize_browser_id(query);
    let candidates = registry::process_name_candidates(query);
    ROUTE_SNAPSHOT.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(snapshot_running_processes());
        }
        let mut snapshot = cell.borrow();
        let running = snapshot.as_ref().expect("process snapshot missing");
        if running.is_empty() {
            drop(snapshot);
            *cell.borrow_mut() = Some(snapshot_running_processes());
            snapshot = cell.borrow();
        }
        let running = snapshot.as_ref().expect("process snapshot missing");
        if candidates
            .iter()
            .any(|candidate| running.iter().any(|proc| process_matches(proc, candidate)))
        {
            return true;
        }
        running_path_markers_match(browser_id, running)
    })
}

fn running_path_markers_match(browser_id: &str, running: &HashSet<String>) -> bool {
    let browser_id = registry::normalize_browser_id(browser_id);
    let markers = registry::running_path_markers(browser_id);
    if markers.is_empty() {
        return false;
    }
    running.iter().any(|proc| {
        markers
            .iter()
            .any(|marker| process_matches(proc, marker) || proc.contains(marker.as_str()))
    })
}

fn snapshot_running_processes() -> HashSet<String> {
    #[cfg(target_os = "macos")]
    {
        macos::snapshot_running_processes()
    }
    #[cfg(target_os = "windows")]
    {
        windows::snapshot_running_processes()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        linux::snapshot_running_processes()
    }
}

fn normalize_process_name(name: &str) -> String {
    let trimmed = name.trim();
    let base = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let lower = base.to_ascii_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

fn process_matches(running: &str, candidate: &str) -> bool {
    let running = normalize_process_name(running);
    let candidate = normalize_process_name(candidate);
    if running.is_empty() || candidate.is_empty() {
        return false;
    }
    if running == candidate {
        return true;
    }
    // Browser helpers (e.g. "Microsoft Edge Helper") — prefix only, not substring.
    running.starts_with(&format!("{candidate} "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_paths_and_exe_suffix() {
        assert_eq!(
            normalize_process_name(r"C:\Program Files\Microsoft\Edge\msedge.exe"),
            "msedge"
        );
        assert_eq!(
            normalize_process_name("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            "google chrome"
        );
    }

    #[test]
    fn process_matches_helpers_and_basenames() {
        assert!(process_matches("Microsoft Edge Helper", "Microsoft Edge"));
        assert!(process_matches("msedge", "msedge"));
        assert!(!process_matches("supersurfer", "edge"));
        assert!(!process_matches("msedgewebview2_crashpad_handler", "edge"));
        assert!(!process_matches(
            "msedgewebview2_crashpad_handler",
            "msedge"
        ));
        assert!(!process_matches("knowledge-agent", "edge"));
    }

    #[test]
    fn running_path_markers_detect_edge_app_bundle_paths() {
        let running = HashSet::from([
            "/applications/microsoft edge.app/contents/macos/microsoft edge".to_string(),
            "microsoft edge helper (renderer".to_string(),
        ]);
        assert!(running_path_markers_match("edge", &running));
        assert!(running_path_markers_match("Microsoft Edge", &running));
    }

    #[test]
    fn is_running_uses_injected_snapshot() {
        replace_snapshot_for_tests(HashSet::from([
            "msedge".to_string(),
            "microsoft edge helper".to_string(),
        ]));
        assert!(is_running("edge"));
        assert!(is_running("Microsoft Edge"));
        assert!(!is_running("firefox"));
    }

    #[test]
    fn route_guard_clears_snapshot_after_route() {
        replace_snapshot_for_tests(HashSet::from(["msedge".to_string()]));
        assert!(is_running("edge"));
        drop(RouteProcessGuard::new());
        ROUTE_SNAPSHOT.with(|cell| assert!(cell.borrow().is_none()));
    }
}
