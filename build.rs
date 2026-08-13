fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }

    if let Err(err) = winresource::WindowsResource::new().compile() {
        if std::env::var_os("CI").is_some() {
            panic!("failed to embed Windows version resource: {err}");
        }
        println!(
            "cargo:warning=Windows version resource not embedded ({err}); \
             install mingw-w64 windres (x86_64-w64-mingw32-windres) for release EXEs"
        );
    }
}
