use libc::pid_t;
use std::collections::HashSet;
use std::mem;
use std::ptr;

const PROC_ALL_PIDS: u32 = 1;
const PROC_NAME_LEN: usize = 256;

#[link(name = "proc", kind = "dylib")]
extern "C" {
    fn proc_listpids(
        type_: u32,
        typeinfo: u32,
        buffer: *mut libc::c_void,
        buffersize: libc::c_int,
    ) -> libc::c_int;
    fn proc_name(pid: pid_t, buffer: *mut libc::c_void, buffersize: u32) -> libc::c_int;
}

pub fn snapshot_running_processes() -> HashSet<String> {
    let nbytes = unsafe { proc_listpids(PROC_ALL_PIDS, 0, ptr::null_mut(), 0) };
    if nbytes <= 0 {
        return HashSet::new();
    }

    let mut pid_buf = vec![0u8; nbytes as usize];
    let ret = unsafe { proc_listpids(PROC_ALL_PIDS, 0, pid_buf.as_mut_ptr().cast(), nbytes) };
    if ret <= 0 {
        return HashSet::new();
    }

    let pid_size = mem::size_of::<pid_t>();
    let mut running = HashSet::new();
    let mut name_buf = [0u8; PROC_NAME_LEN];

    for chunk in pid_buf.chunks_exact(pid_size) {
        let pid = pid_t::from_ne_bytes(chunk.try_into().expect("pid chunk"));
        if pid <= 0 {
            continue;
        }
        let len = unsafe { proc_name(pid, name_buf.as_mut_ptr().cast(), name_buf.len() as u32) };
        if len <= 0 {
            continue;
        }
        let end = len as usize;
        let name = String::from_utf8_lossy(&name_buf[..end.min(name_buf.len())]).into_owned();
        if !name.is_empty() {
            running.insert(name.to_ascii_lowercase());
        }
    }
    running
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libproc_snapshot_returns_processes() {
        let running = snapshot_running_processes();
        assert!(!running.is_empty(), "expected at least one running process");
    }
}
