use crate::config::cache;
use crate::config::prepare;
use crate::script::runtime::ScriptRuntime;
use anyhow::Result;
use std::path::Path;

pub struct LoadedConfig {
    pub runtime: ScriptRuntime,
    pub source_path: std::path::PathBuf,
    /// True when the config script references ctx.opener / context.opener.
    pub references_opener: bool,
}

pub fn load_config(path: &Path) -> Result<LoadedConfig> {
    let source = super::read_config_source(path)?;
    let cache_dir = super::cache_dir()?;
    let helpers = ScriptRuntime::helpers_prelude();
    let key = cache::cache_key(&source, path, helpers);

    let js = match cache::read_cached(&cache_dir, &key)? {
        Some(cached) => cached,
        None => {
            let prepared = prepare::prepare_config_source(&source);
            let wrapped = wrap_config_script(&prepared);
            cache::write_cached(&cache_dir, &key, &wrapped)?;
            wrapped
        }
    };

    let runtime = ScriptRuntime::from_js(&js)?;
    Ok(LoadedConfig {
        runtime,
        source_path: path.to_path_buf(),
        references_opener: references_opener_context(&js),
    })
}

fn references_opener_context(js: &str) -> bool {
    js.contains("ctx.opener") || js.contains("context.opener")
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
    format!("{}{}", ScriptRuntime::helpers_prelude(), user_js)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_opener_context_references() {
        assert!(references_opener_context(
            "match: (url, ctx) => ctx.opener?.name === 'Slack'"
        ));
        assert!(!references_opener_context(
            "match: (url) => url.hostname === 'example.com'"
        ));
    }
}
