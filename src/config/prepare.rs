pub fn prepare_config_source(source: &str) -> String {
    source.replace("export default", "globalThis.__SUPERSURFER_CONFIG__ =")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_export_default() {
        let src = r#"/** @type {import('./supersurfer').RouterConfig} */
export default { defaultBrowser: "safari" };"#;
        let js = prepare_config_source(src);
        assert!(js.contains("globalThis.__SUPERSURFER_CONFIG__"));
        assert!(!js.contains("export default"));
        assert!(js.contains("@type"));
    }
}