use crate::config::cache;
use crate::config::transpile;
use crate::script::runtime::ScriptRuntime;
use anyhow::Result;
use std::path::Path;

pub struct LoadedConfig {
    pub runtime: ScriptRuntime,
    pub source_path: std::path::PathBuf,
}

pub fn load_config(path: &Path) -> Result<LoadedConfig> {
    let source = super::read_config_source(path)?;
    let cache_dir = super::cache_dir()?;
    let key = cache::cache_key(&source, path);

    let js = match cache::read_cached(&cache_dir, &key)? {
        Some(cached) => cached,
        None => {
            let transpiled = if path.extension().and_then(|e| e.to_str()) == Some("ts") {
                transpile::transpile(&source, path)?
            } else {
                source
                    .replace("export default", "globalThis.__SUPERSURFER_CONFIG__ =")
            };
            let wrapped = wrap_config_script(&transpiled);
            cache::write_cached(&cache_dir, &key, &wrapped)?;
            wrapped
        }
    };

    let runtime = ScriptRuntime::from_js(&js)?;
    Ok(LoadedConfig {
        runtime,
        source_path: path.to_path_buf(),
    })
}

pub fn load_default_config() -> Result<LoadedConfig> {
    let path = super::config_path()?;
    if !path.exists() {
        anyhow::bail!(
            "no config found. Run `supersurfer init` to create one at {}",
            path.display()
        );
    }
    load_config(&path)
}

fn wrap_config_script(user_js: &str) -> String {
    format!(
        "{}{}",
        ScriptRuntime::helpers_prelude(),
        user_js
    )
}
