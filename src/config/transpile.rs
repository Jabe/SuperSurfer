use anyhow::Result;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

static IMPORT_TYPE: OnceLock<Regex> = OnceLock::new();
static SATISFIES: OnceLock<Regex> = OnceLock::new();

pub fn transpile(source: &str, _path: &Path) -> Result<String> {
    let mut out = source.to_string();

    let import_type = IMPORT_TYPE.get_or_init(|| Regex::new(r"(?m)^import\s+type\s+.+$").unwrap());
    out = import_type.replace_all(&out, "").to_string();

    let satisfies = SATISFIES.get_or_init(|| Regex::new(r"\s+satisfies\s+[\w.<>,\s\[\]|]+").unwrap());
    out = satisfies.replace_all(&out, "").to_string();

    out = out.replace("export default", "globalThis.__SUPERSURFER_CONFIG__ =");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_import_type_and_satisfies() {
        let src = r#"import type { RouterConfig } from "./supersurfer";
export default { defaultBrowser: "safari" } satisfies RouterConfig;"#;
        let js = transpile(src, Path::new("config.ts")).unwrap();
        assert!(js.contains("globalThis.__SUPERSURFER_CONFIG__"));
        assert!(!js.contains("import type"));
        assert!(!js.contains("satisfies"));
    }
}
