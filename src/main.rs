// Release Windows builds use the GUI subsystem so default-browser URL launches
// don't flash a console window. CLI mode attaches to the parent console when present.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;

fn main() -> Result<()> {
    supersurfer::cli::run()
}
