use crate::context::Context as RouteContext;
use anyhow::{anyhow, Context as _, Result};
use globset::{Glob, GlobMatcher};
use rquickjs::context::intrinsic;
use rquickjs::{Array, CatchResultExt, Context, Function, Object, Runtime, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use url::Url;

const EVAL_TIMEOUT: Duration = Duration::from_millis(250);

static GLOB_MATCHERS: LazyLock<Mutex<HashMap<String, GlobMatcher>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct EvalBudget {
    started: Mutex<Instant>,
}

fn new_runtime() -> Result<(Runtime, Arc<EvalBudget>)> {
    let runtime = Runtime::new().context("failed to create QuickJS runtime")?;
    let budget = Arc::new(EvalBudget {
        started: Mutex::new(Instant::now()),
    });
    let budget_for_interrupt = Arc::clone(&budget);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        budget_for_interrupt
            .started
            .lock()
            .is_ok_and(|started| started.elapsed() > EVAL_TIMEOUT)
    })));
    Ok((runtime, budget))
}

fn reset_budget(budget: &EvalBudget) {
    if let Ok(mut started) = budget.started.lock() {
        *started = Instant::now();
    }
}

fn create_sandbox_context(runtime: &Runtime) -> Result<Context> {
    let ctx = Context::builder()
        .with::<intrinsic::Date>()
        .with::<intrinsic::Eval>()
        .with::<intrinsic::RegExpCompiler>()
        .with::<intrinsic::RegExp>()
        .with::<intrinsic::Json>()
        .with::<intrinsic::MapSet>()
        .with::<intrinsic::TypedArrays>()
        .with::<intrinsic::BigInt>()
        .build(runtime)
        .context("failed to create QuickJS context")?;
    ctx.with(|ctx| -> Result<()> {
        // Eval intrinsic is required for ctx.eval() from Rust; hide eval() from config scripts.
        ctx.globals().remove("eval")?;
        Ok(())
    })?;
    Ok(ctx)
}

pub struct ScriptRuntime {
    _runtime: Runtime,
    ctx: Context,
    budget: Arc<EvalBudget>,
}

#[derive(Debug, Clone)]
pub struct BrowserTarget {
    pub name: Option<String>,
    pub private: bool,
    pub app: Option<String>,
    pub exe: Option<String>,
}

impl ScriptRuntime {
    pub fn from_js(js: &str) -> Result<Self> {
        let (runtime, budget) = new_runtime()?;
        let ctx = create_sandbox_context(&runtime)?;

        reset_budget(&budget);
        ctx.with(|ctx| -> Result<()> {
            let globals = ctx.globals();
            install_host_functions(&globals)?;
            ctx.eval::<(), _>(js.as_bytes())
                .catch(&ctx)
                .map_err(|e| anyhow!("config script failed to evaluate: {e}"))?;
            Ok(())
        })?;

        Ok(Self {
            _runtime: runtime,
            ctx,
            budget,
        })
    }

    pub fn helpers_prelude() -> &'static str {
        include_str!("helpers.js")
    }

    pub fn default_browser(&self) -> Result<String> {
        self.ctx.with(|ctx| {
            let config: Object = ctx
                .globals()
                .get("__SUPERSURFER_CONFIG__")
                .context("config missing __SUPERSURFER_CONFIG__ export")?;
            config
                .get("defaultBrowser")
                .context("config missing defaultBrowser")
        })
    }

    pub fn url_cleaning_mode(&self) -> Result<String> {
        self.ctx.with(|ctx| {
            let config: Object = ctx.globals().get("__SUPERSURFER_CONFIG__")?;
            let mode: Option<String> = config.get("urlCleaning").ok();
            Ok(mode.unwrap_or_else(|| "default".to_string()))
        })
    }

    pub fn route(&self, url: &Url, context: &RouteContext) -> Result<(Option<BrowserTarget>, Url)> {
        reset_budget(&self.budget);
        let working = RefCell::new(url.clone());
        let target = self.route_inner(&working, context)?;
        Ok((target, working.into_inner()))
    }

    fn route_inner(
        &self,
        url: &RefCell<Url>,
        context: &RouteContext,
    ) -> Result<Option<BrowserTarget>> {
        self.ctx.with(|ctx| {
            let config: Object = ctx.globals().get("__SUPERSURFER_CONFIG__")?;
            if let Ok(rules) = config.get::<_, Array>("rewrite") {
                apply_rewrite(&ctx, &rules, url, context)?;
            }

            let current = url.borrow();
            let handlers: Array = config.get("handlers").context("config missing handlers")?;
            let len = handlers.len();

            for i in 0..len {
                let handler: Object = handlers.get(i)?;
                let browser_value: Value = handler.get("browser")?;
                let matcher: Value = handler.get("match")?;
                if eval_match(&ctx, matcher, &current, context)? {
                    return Ok(Some(resolve_browser_target(&ctx, browser_value, &current)?));
                }
            }
            Ok(None)
        })
    }
}

fn apply_rewrite<'js>(
    ctx: &rquickjs::Ctx<'js>,
    rules: &Array<'js>,
    url: &RefCell<Url>,
    context: &RouteContext,
) -> Result<()> {
    let len = rules.len();
    for i in 0..len {
        let rule: Object = rules.get(i)?;
        let matcher: Value = rule.get("match")?;
        if eval_match(ctx, matcher, &url.borrow(), context)? {
            let transform: Function = rule.get("url")?;
            let url_obj = make_url_object(ctx, &url.borrow())?;
            match transform.call::<_, Value>((url_obj,)) {
                Ok(result) => {
                    if let Some(next) = result.as_string() {
                        if let Ok(parsed) = Url::parse(&next.to_string()?) {
                            *url.borrow_mut() = parsed;
                        }
                    }
                }
                Err(err) => eprintln!("rewrite rule skipped: {err}"),
            }
        }
    }
    Ok(())
}

fn eval_match<'js>(
    ctx: &rquickjs::Ctx<'js>,
    matcher: Value<'js>,
    url: &Url,
    context: &RouteContext,
) -> Result<bool> {
    let eval_match_fn: Function = ctx.globals().get("__evalMatch")?;
    let url_obj = make_url_object(ctx, url)?;
    let ctx_obj = make_context_object(ctx, context)?;
    eval_match_fn
        .call::<_, bool>((matcher, url_obj, ctx_obj))
        .context("matcher evaluation failed")
}

fn resolve_browser_target<'js>(
    ctx: &rquickjs::Ctx<'js>,
    value: Value<'js>,
    url: &Url,
) -> Result<BrowserTarget> {
    if value.is_function() {
        let func = value.as_function().context("expected browser function")?;
        let url_obj = make_url_object(ctx, url)?;
        let result: Value = func.call((url_obj,))?;
        return parse_browser_target(result);
    }
    parse_browser_target(value)
}

fn parse_browser_target(value: Value<'_>) -> Result<BrowserTarget> {
    if let Some(name) = value.as_string() {
        return Ok(BrowserTarget {
            name: Some(map_browser_name(&name.to_string()?)?),
            private: false,
            app: None,
            exe: None,
        });
    }

    let obj = value
        .as_object()
        .context("browser target must be a string or object")?;

    if let Ok(display_name) = obj.get::<_, String>("name") {
        let profile: Option<String> = obj.get("profile").ok();
        return Ok(BrowserTarget {
            name: Some(browser_spec_from_parts(&display_name, profile)?),
            private: false,
            app: None,
            exe: None,
        });
    }

    let browser: Option<String> = obj.get("browser").ok();
    let private: bool = obj.get("private").unwrap_or(false);
    let app: Option<String> = obj.get("app").ok();
    let exe: Option<String> = obj.get("exe").ok();
    let name = browser
        .map(|browser| {
            let profile: Option<String> = obj.get("profile").ok();
            browser_spec_from_parts(&browser, profile)
        })
        .transpose()?;

    Ok(BrowserTarget {
        name,
        private,
        app,
        exe,
    })
}

fn browser_spec_from_parts(browser: &str, profile: Option<String>) -> Result<String> {
    let id = map_browser_name(browser)?;
    Ok(match profile {
        Some(profile) => format!("{id}:{profile}"),
        None => id,
    })
}

fn map_browser_name(display_name: &str) -> Result<String> {
    if display_name.to_lowercase().ends_with(".app") {
        return Ok(display_name.to_string());
    }
    Ok(crate::browser::registry::normalize_browser_id(display_name).to_string())
}

fn install_host_functions(globals: &Object<'_>) -> Result<()> {
    globals.set(
        "__domainMatch",
        Function::new(globals.ctx().clone(), |hostname: String, domain: String| {
            domain_match(&hostname, &domain)
        })?,
    )?;
    globals.set(
        "__globMatch",
        Function::new(globals.ctx().clone(), |pattern: String, target: String| {
            glob_match(&pattern, &target)
        })?,
    )?;
    globals.set(
        "__pathMatch",
        Function::new(globals.ctx().clone(), |pattern: String, path: String| {
            path_match(&pattern, &path)
        })?,
    )?;
    Ok(())
}

fn domain_match(hostname: &str, domain: &str) -> bool {
    if hostname == domain {
        return true;
    }
    hostname == domain.strip_prefix('.').unwrap_or(domain)
        || hostname.ends_with(&format!(".{domain}"))
}

fn glob_match(pattern: &str, target: &str) -> bool {
    let mut guard = GLOB_MATCHERS.lock().unwrap();
    let matcher = guard
        .entry(pattern.to_string())
        .or_insert_with(|| Glob::new(pattern).unwrap().compile_matcher());
    matcher.is_match(target)
}

fn path_match(pattern: &str, path: &str) -> bool {
    glob_match(pattern, path)
}

fn make_url_object<'js>(ctx: &rquickjs::Ctx<'js>, url: &Url) -> Result<Object<'js>> {
    let obj = Object::new(ctx.clone())?;
    obj.set("href", url.as_str())?;
    obj.set("protocol", url.scheme())?;
    obj.set("hostname", url.host_str().unwrap_or(""))?;
    obj.set("port", url.port().unwrap_or_default())?;
    obj.set("pathname", url.path())?;
    obj.set(
        "search",
        url.query().map(|q| format!("?{q}")).unwrap_or_default(),
    )?;
    let search_params = Object::new(ctx.clone())?;
    for (key, value) in url.query_pairs() {
        search_params.set(key.as_ref(), value.as_ref())?;
    }
    obj.set("searchParams", search_params)?;
    Ok(obj)
}

fn make_context_object<'js>(
    ctx: &rquickjs::Ctx<'js>,
    context: &RouteContext,
) -> Result<Object<'js>> {
    let obj = Object::new(ctx.clone())?;
    if let Some(opener) = &context.opener {
        let opener_obj = Object::new(ctx.clone())?;
        opener_obj.set("name", opener.name.as_str())?;
        if let Some(bundle_id) = &opener.bundle_id {
            opener_obj.set("bundleId", bundle_id.as_str())?;
        }
        if let Some(path) = &opener.path {
            opener_obj.set("path", path.as_str())?;
        }
        obj.set("opener", opener_obj)?;
    }
    let platform = match context.platform {
        crate::context::Platform::Macos => "macos",
        crate::context::Platform::Windows => "windows",
        crate::context::Platform::Linux => "linux",
    };
    obj.set("platform", platform)?;
    let modifiers = Object::new(ctx.clone())?;
    modifiers.set("shift", context.modifiers.shift)?;
    modifiers.set("alt", context.modifiers.alt)?;
    modifiers.set("ctrl", context.modifiers.ctrl)?;
    modifiers.set("cmd", context.modifiers.cmd)?;
    obj.set("modifiers", modifiers)?;
    Ok(obj)
}

#[cfg(test)]
mod sandbox_tests {
    use super::*;
    use rquickjs::Runtime;

    fn probe_typeof(name: &str) -> Result<String> {
        let runtime = Runtime::new()?;
        let ctx = create_sandbox_context(&runtime)?;
        let js = format!(
            "{}{}\nglobalThis.__SUPERSURFER_CONFIG__ = {{ defaultBrowser: \"chrome\", handlers: [] }};",
            ScriptRuntime::helpers_prelude(),
            ""
        );
        ctx.with(|ctx| -> Result<()> {
            install_host_functions(&ctx.globals())?;
            ctx.eval::<(), _>(js.as_bytes())?;
            Ok(())
        })?;
        ctx.with(|ctx| {
            let ty: String = ctx.eval(format!("typeof {name}").into_bytes())?;
            Ok(ty)
        })
    }

    #[test]
    fn sandbox_blocks_network_and_node_apis() {
        for api in ["fetch", "require", "os", "std", "process", "Deno"] {
            let ty = probe_typeof(api).unwrap_or_else(|_| "error".into());
            assert_eq!(ty, "undefined", "{api} should be unavailable");
        }
    }

    #[test]
    fn sandbox_exposes_expected_helpers() {
        for api in ["host", "domain", "glob", "__evalMatch", "__domainMatch"] {
            let ty = probe_typeof(api).unwrap();
            assert_eq!(ty, "function", "{api} should be a function");
        }
    }

    #[test]
    fn sandbox_blocks_eval_promise_proxy() {
        for api in ["eval", "Promise", "Proxy"] {
            assert_eq!(
                probe_typeof(api).unwrap(),
                "undefined",
                "{api} should be blocked"
            );
        }
        assert_eq!(probe_typeof("JSON").unwrap(), "object");
        assert_eq!(probe_typeof("Function").unwrap(), "function");
    }

    #[test]
    fn sandbox_has_no_console() {
        assert_eq!(probe_typeof("console").unwrap(), "undefined");
    }

    #[test]
    fn sandbox_has_no_timers() {
        for api in ["setTimeout", "setInterval"] {
            assert_eq!(probe_typeof(api).unwrap(), "undefined");
        }
    }

    #[test]
    fn malicious_config_load_fails_cleanly() {
        let js = format!(
            "{}{}",
            ScriptRuntime::helpers_prelude(),
            "throw new Error('bad config');"
        );
        assert!(ScriptRuntime::from_js(&js).is_err());
    }

    #[test]
    fn rewrite_errors_are_non_fatal() {
        let js = format!(
            r#"{}{}
globalThis.__SUPERSURFER_CONFIG__ = {{
  defaultBrowser: "chrome",
  handlers: [],
  rewrite: [{{ match: host("example.com"), url: () => {{ throw new Error("boom"); }} }}],
}};"#,
            ScriptRuntime::helpers_prelude(),
            ""
        );
        let rt = ScriptRuntime::from_js(&js).unwrap();
        let url = Url::parse("https://example.com/path").unwrap();
        let ctx = RouteContext::default();
        let (target, out) = rt.route(&url, &ctx).unwrap();
        assert!(target.is_none());
        assert_eq!(out.as_str(), "https://example.com/path");
    }

    #[test]
    fn matcher_errors_fail_route() {
        let js = format!(
            r#"{}{}
globalThis.__SUPERSURFER_CONFIG__ = {{
  defaultBrowser: "chrome",
  handlers: [{{ match: () => {{ throw new Error("boom"); }}, browser: "firefox" }}],
}};"#,
            ScriptRuntime::helpers_prelude(),
            ""
        );
        let rt = ScriptRuntime::from_js(&js).unwrap();
        let url = Url::parse("https://example.com").unwrap();
        let ctx = RouteContext::default();
        let err = rt.route(&url, &ctx).unwrap_err().to_string();
        assert!(err.contains("matcher evaluation failed"));
    }

    #[test]
    fn route_interrupts_long_running_matchers() {
        let js = format!(
            r#"{}{}
globalThis.__SUPERSURFER_CONFIG__ = {{
  defaultBrowser: "chrome",
  handlers: [{{ match: () => {{ while (true) {{}} }}, browser: "firefox" }}],
}};"#,
            ScriptRuntime::helpers_prelude(),
            ""
        );
        let rt = ScriptRuntime::from_js(&js).unwrap();
        let url = Url::parse("https://example.com").unwrap();
        let ctx = RouteContext::default();
        let started = Instant::now();
        assert!(rt.route(&url, &ctx).is_err());
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "interrupt handler should stop runaway matchers quickly"
        );
    }
}
