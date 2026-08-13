use std::env;
use std::error::Error;
use std::process;

use editpe::types::{FixedFileInfo, VersionU16, VersionU32};
use editpe::{Image, VersionInfo, VersionStringTable};

fn main() {
    let mut args = env::args().skip(1);
    let Some(exe) = args.next() else {
        usage();
    };
    let Some(version) = args.next() else {
        usage();
    };

    if let Err(err) = embed(&exe, &version) {
        eprintln!("embed-version: {err}");
        process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("usage: embed-version <supersurfer.exe> <version>");
    process::exit(2);
}

fn parse_version(raw: &str) -> Result<(u16, u16, u16, u16), String> {
    let mut parts = raw.split('.');
    let mut next = |required: bool| -> Result<u16, String> {
        match parts.next() {
            Some(part) => part
                .parse()
                .map_err(|_| format!("invalid version component in {raw:?}")),
            None if required => Err(format!("invalid version {raw:?}")),
            None => Ok(0),
        }
    };
    Ok((next(true)?, next(false)?, next(false)?, next(false)?))
}

fn packed(major: u16, minor: u16, patch: u16, build: u16) -> VersionU32 {
    VersionU32 {
        major: (u32::from(major) << 16) | u32::from(minor),
        minor: (u32::from(patch) << 16) | u32::from(build),
    }
}

fn embed(path: &str, version: &str) -> Result<(), Box<dyn Error>> {
    let (major, minor, patch, build) = parse_version(version)?;
    let file_version = packed(major, minor, patch, build);

    let mut strings = VersionStringTable {
        key: "040904b0".into(),
        strings: Default::default(),
    };
    for (key, value) in [
        ("CompanyName", "Jan Berdel"),
        (
            "FileDescription",
            "Cross-platform default browser router with JavaScript config",
        ),
        ("FileVersion", version),
        ("InternalName", "SuperSurfer"),
        ("LegalCopyright", "Copyright (c) 2026 Jan Berdel"),
        ("OriginalFilename", "supersurfer.exe"),
        ("ProductName", "SuperSurfer"),
        ("ProductVersion", version),
    ] {
        strings.strings.insert(key.into(), value.into());
    }

    let info = VersionInfo {
        info: FixedFileInfo {
            file_version,
            product_version: file_version,
            ..FixedFileInfo::default()
        },
        strings: vec![strings],
        vars: vec![VersionU16 {
            major: 0x0409,
            minor: 0x04b0,
        }],
    };

    let mut image = Image::parse_file(path)?;
    let mut resources = image.resource_directory().cloned().unwrap_or_default();
    resources.set_version_info(&info)?;
    image.set_resource_directory(resources)?;
    image.write_file(path)?;
    Ok(())
}
