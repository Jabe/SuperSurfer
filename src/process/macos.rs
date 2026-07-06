use libc::pid_t;
use std::collections::HashSet;
use std::mem;
use std::path::Path;
use std::ptr;

const PROC_ALL_PIDS: u32 = 1;
const PROC_NAME_LEN: usize = 256;
const PROC_PIDPATHINFO_MAXSIZE: u32 = 4096;

#[link(name = "proc", kind = "dylib")]
extern "C" {
    fn proc_listpids(
        type_: u32,
        typeinfo: u32,
        buffer: *mut libc::c_void,
        buffersize: libc::c_int,
    ) -> libc::c_int;
    fn proc_name(pid: pid_t, buffer: *mut libc::c_void, buffersize: u32) -> libc::c_int;
    fn proc_pidpath(pid: pid_t, buffer: *mut libc::c_void, buffersize: u32) -> libc::c_int;
}

pub fn snapshot_running_processes() -> HashSet<String> {
    let pids = list_pids();
    let mut running = HashSet::new();
    let mut name_buf = [0u8; PROC_NAME_LEN];
    let mut path_buf = [0u8; PROC_PIDPATHINFO_MAXSIZE as usize];

    for pid in pids {
        let len = unsafe { proc_name(pid, name_buf.as_mut_ptr().cast(), name_buf.len() as u32) };
        if len > 0 {
            let end = len as usize;
            let name = String::from_utf8_lossy(&name_buf[..end.min(name_buf.len())]).into_owned();
            if !name.is_empty() {
                running.insert(name.to_ascii_lowercase());
            }
        }

        let path_len =
            unsafe { proc_pidpath(pid, path_buf.as_mut_ptr().cast(), path_buf.len() as u32) };
        if path_len > 0 {
            let end = path_len as usize;
            let path = String::from_utf8_lossy(&path_buf[..end.min(path_buf.len())]).into_owned();
            extend_with_path_entries(&mut running, &path);
        }
    }

    running
}

fn list_pids() -> Vec<pid_t> {
    let nbytes = unsafe { proc_listpids(PROC_ALL_PIDS, 0, ptr::null_mut(), 0) };
    if nbytes <= 0 {
        return Vec::new();
    }

    let mut pid_buf = vec![0u8; nbytes as usize];
    let ret = unsafe { proc_listpids(PROC_ALL_PIDS, 0, pid_buf.as_mut_ptr().cast(), nbytes) };
    if ret <= 0 {
        return Vec::new();
    }

    let pid_size = mem::size_of::<pid_t>();
    pid_buf
        .chunks_exact(pid_size)
        .filter_map(|chunk| {
            let pid = pid_t::from_ne_bytes(chunk.try_into().ok()?);
            (pid > 0).then_some(pid)
        })
        .collect()
}

fn extend_with_path_entries(running: &mut HashSet<String>, path: &str) {
    let lower = path.to_ascii_lowercase();
    if lower.is_empty() {
        return;
    }
    running.insert(lower.clone());

    if let Some(base) = Path::new(&lower).file_name().and_then(|s| s.to_str()) {
        running.insert(base.to_string());
    }

    for segment in lower.split('/') {
        if segment.ends_with(".app") {
            running.insert(segment.trim_end_matches(".app").to_string());
            running.insert(segment.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libproc_snapshot_returns_processes() {
        let running = snapshot_running_processes();
        assert!(!running.is_empty(), "expected at least one running process");
    }

    #[test]
    fn extend_with_path_entries_extracts_app_bundle_name() {
        let mut running = HashSet::new();
        extend_with_path_entries(
            &mut running,
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        );
        assert!(running.contains("microsoft edge"));
        assert!(running.contains("microsoft edge.app"));
    }
}
