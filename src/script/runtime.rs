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
            // urlCleaning may be a string ("off" | "default") or, per the type
            // declaration, a custom-rules array. Custom rules are not yet
            // implemented; fall back to "default" and warn so the user isn't
            // silently misled into thinking their rules are applied.
            //
            // Inspect the raw value rather than coercing to String: a String
            // FromJs of a non-string (e.g. an array) yields None, which is
            // indistinguishable from the key being absent and would skip the
            // warning for exactly the array case we want to catch.
            let value: Value = config.get("urlCleaning")?;
            if value.is_undefined() || value.is_null() {
                return Ok("default".to_string());
            }
            if let Some(s) = value.as_string() {
                let mode = s.to_string()?;
                if mode == "off" || mode == "default" {
                    return Ok(mode);
                }
                eprintln!(
                    "urlCleaning: unsupported value {mode:?} (expected \"off\" or \"default\"). \
                     Custom rule arrays are not yet implemented; falling back to \"default\"."
                );
                return Ok("default".to_string());
            }
            eprintln!(
                "urlCleaning: unsupported value (expected the string \"off\" or \"default\"). \
                 Custom rule arrays are not yet implemented; falling back to \"default\"."
            );
            Ok("default".to_string())
        })
    }

    pub fn route(&self, url: &Url, context: &RouteContext) -> Result<(Option<BrowserTarget>, Url)> {
        let _process_guard = crate::process::RouteProcessGuard::new();
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
            let original = url.borrow().clone();
            let url_obj = make_mutable_url_object(ctx, &original)?;
            match transform.call::<_, Value>((url_obj.clone(),)) {
                Ok(result) => {
                    if let Some(next) = result.as_string() {
                        // Returned a replacement URL string (Finicky style).
                        if let Ok(parsed) = Url::parse(&next.to_string()?) {
                            *url.borrow_mut() = parsed;
                        }
                    } else {
                        // Mutated the URL object in place (e.g. searchParams.delete).
                        match read_back_url(&original, &url_obj) {
                            Ok(parsed) => *url.borrow_mut() = parsed,
                            Err(err) => eprintln!("rewrite rule produced invalid URL: {err}"),
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
        });
    }

    let browser: Option<String> = obj.get("browser").ok();
    let private: bool = obj.get("private").unwrap_or(false);
    let name = browser
        .map(|browser| {
            let profile: Option<String> = obj.get("profile").ok();
            browser_spec_from_parts(&browser, profile)
        })
        .transpose()?;

    Ok(BrowserTarget { name, private })
}

fn browser_spec_from_parts(browser: &str, profile: Option<String>) -> Result<String> {
    let id = map_browser_name(browser)?;
    Ok(match profile {
        Some(profile) => format!("{id}:{profile}"),
        None => id,
    })
}

fn map_browser_name(display_name: &str) -> Result<String> {
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
    globals.set(
        "__consoleLog",
        Function::new(globals.ctx().clone(), |message: String| {
            crate::logging::append_script_log(&message).ok();
        })?,
    )?;
    globals.set(
        "__processRunning",
        Function::new(globals.ctx().clone(), |name: String| {
            crate::process::is_running(&name)
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
    // Recover from a poisoned lock rather than panicking inside a QuickJS host
    // callback (which would unwind through C frames and abort the process).
    let mut guard = GLOB_MATCHERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(matcher) = guard.get(pattern) {
        return matcher.is_match(target);
    }
    // A malformed pattern from user config must degrade to "no match" so routing
    // can fall back to the default browser, not abort on every link click.
    match Glob::new(pattern) {
        Ok(glob) => {
            let matcher = glob.compile_matcher();
            let is_match = matcher.is_match(target);
            guard.insert(pattern.to_string(), matcher);
            is_match
        }
        Err(err) => {
            crate::logging::append_script_log(&format!("invalid glob pattern {pattern:?}: {err}"))
                .ok();
            false
        }
    }
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
    obj.set("searchParams", make_search_params(ctx, url)?)?;
    Ok(obj)
}

/// Build a `URLSearchParams` instance (from helpers.js) so config code can call
/// `.get()`, `.has()`, `.getAll()` on it, matching the documented `URL` API.
fn make_search_params<'js>(ctx: &rquickjs::Ctx<'js>, url: &Url) -> Result<Object<'js>> {
    let pairs = query_pairs_array(ctx, url)?;
    let factory: Function = ctx.globals().get("__makeSearchParams")?;
    Ok(factory.call((pairs,))?)
}

fn query_pairs_array<'js>(ctx: &rquickjs::Ctx<'js>, url: &Url) -> Result<Array<'js>> {
    let pairs = Array::new(ctx.clone())?;
    for (i, (key, value)) in url.query_pairs().enumerate() {
        let pair = Array::new(ctx.clone())?;
        pair.set(0, key.as_ref())?;
        pair.set(1, value.as_ref())?;
        pairs.set(i, pair)?;
    }
    Ok(pairs)
}

/// Build a mutable URL object for `rewrite` rules: `searchParams` supports
/// `delete/set/append`, and direct writes to `pathname`, `hostname`, etc. are
/// read back afterwards by [`read_back_url`].
fn make_mutable_url_object<'js>(ctx: &rquickjs::Ctx<'js>, url: &Url) -> Result<Object<'js>> {
    let parts = Object::new(ctx.clone())?;
    parts.set("protocol", format!("{}:", url.scheme()))?;
    parts.set("username", url.username())?;
    parts.set("password", url.password().unwrap_or(""))?;
    parts.set("hostname", url.host_str().unwrap_or(""))?;
    parts.set(
        "port",
        url.port().map(|p| p.to_string()).unwrap_or_default(),
    )?;
    parts.set("pathname", url.path())?;
    parts.set(
        "hash",
        url.fragment().map(|f| format!("#{f}")).unwrap_or_default(),
    )?;
    parts.set("pairs", query_pairs_array(ctx, url)?)?;
    let factory: Function = ctx.globals().get("__makeMutableUrl")?;
    Ok(factory.call((parts,))?)
}

/// Apply the (possibly mutated) JS URL object back onto a Rust [`Url`], starting
/// from `original` so untouched components (userinfo, etc.) are preserved.
fn read_back_url(original: &Url, obj: &Object<'_>) -> Result<Url> {
    let mut url = original.clone();

    let protocol: String = obj.get("protocol").unwrap_or_default();
    let scheme = protocol.trim_end_matches(':');
    if !scheme.is_empty() && scheme != url.scheme() {
        let _ = url.set_scheme(scheme);
    }

    let hostname: String = obj.get("hostname").unwrap_or_default();
    if !hostname.is_empty() && Some(hostname.as_str()) != url.host_str() {
        let _ = url.set_host(Some(&hostname));
    }

    let port: String = obj.get("port").unwrap_or_default();
    if port.is_empty() {
        let _ = url.set_port(None);
    } else if let Ok(p) = port.parse::<u16>() {
        let _ = url.set_port(Some(p));
    }

    let pathname: String = obj.get("pathname").unwrap_or_default();
    url.set_path(&pathname);

    let hash: String = obj.get("hash").unwrap_or_default();
    let fragment = hash.strip_prefix('#').unwrap_or(&hash);
    url.set_fragment((!fragment.is_empty()).then_some(fragment));

    let username: String = obj.get("username").unwrap_or_default();
    let _ = url.set_username(&username);
    let password: String = obj.get("password").unwrap_or_default();
    let _ = url.set_password((!password.is_empty()).then_some(password.as_str()));

    let search_params: Object = obj.get("searchParams")?;
    let pairs: Array = search_params.get("_pairs")?;
    url.set_query(None);
    let len = pairs.len();
    if len > 0 {
        let mut qp = url.query_pairs_mut();
        for i in 0..len {
            let pair: Array = pairs.get(i)?;
            let key: String = pair.get(0)?;
            let value: String = pair.get(1)?;
            qp.append_pair(&key, &value);
        }
    }

    Ok(url)
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
        for api in [
            "host",
            "domain",
            "glob",
            "__evalMatch",
            "__domainMatch",
            "processRunning",
            "__processRunning",
        ] {
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
    fn sandbox_exposes_console_log() {
        assert_eq!(probe_typeof("console").unwrap(), "object");
        assert_eq!(probe_typeof("console.log").unwrap(), "function");
    }

    #[test]
    fn console_log_writes_to_script_log() {
        let js = format!(
            "{}{}\nglobalThis.__SUPERSURFER_CONFIG__ = {{ defaultBrowser: \"chrome\", handlers: [] }};",
            ScriptRuntime::helpers_prelude(),
            ""
        );
        let rt = ScriptRuntime::from_js(&js).unwrap();
        rt.ctx
            .with(|ctx| -> Result<()> {
                ctx.eval::<(), _>("console.log('hello', 42);")?;
                Ok(())
            })
            .unwrap();
        // Re-resolve the log dir (it is created by append_script_log) and read
        // defensively: parallel bootstrap tests temporarily repoint HOME and
        // delete their temp tree, which can race with this read.
        let path = crate::logging::script_log_file().unwrap();
        let content = std::fs::read_to_string(&path).unwrap_or_else(|_| String::new());
        assert!(content.contains("hello"));
        assert!(content.contains("42"));
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

    fn rewrite_url(rule_url_body: &str, input: &str) -> String {
        let js = format!(
            r#"{}{}
globalThis.__SUPERSURFER_CONFIG__ = {{
  defaultBrowser: "chrome",
  handlers: [],
  rewrite: [{{ match: () => true, url: (u) => {{ {rule_url_body} }} }}],
}};"#,
            ScriptRuntime::helpers_prelude(),
            ""
        );
        let rt = ScriptRuntime::from_js(&js).unwrap();
        let url = Url::parse(input).unwrap();
        let ctx = RouteContext::default();
        let (_, out) = rt.route(&url, &ctx).unwrap();
        out.to_string()
    }

    #[test]
    fn rewrite_searchparams_delete_in_place() {
        assert_eq!(
            rewrite_url(
                r#"u.searchParams.delete("ref");"#,
                "https://example.com/p?ref=1&ok=2"
            ),
            "https://example.com/p?ok=2"
        );
    }

    #[test]
    fn rewrite_searchparams_set_and_append() {
        assert_eq!(
            rewrite_url(
                r#"u.searchParams.set("a", "3"); u.searchParams.append("b", "4");"#,
                "https://example.com/?a=1&a=2"
            ),
            "https://example.com/?a=3&b=4"
        );
    }

    #[test]
    fn rewrite_searchparams_get_returns_value() {
        // delete only when the param has a specific value, exercising get().
        assert_eq!(
            rewrite_url(
                r#"if (u.searchParams.get("track") === "yes") u.searchParams.delete("track");"#,
                "https://example.com/?track=yes&keep=1"
            ),
            "https://example.com/?keep=1"
        );
    }

    #[test]
    fn rewrite_can_mutate_pathname() {
        assert_eq!(
            rewrite_url(r#"u.pathname = "/new";"#, "https://example.com/old?x=1"),
            "https://example.com/new?x=1"
        );
    }

    #[test]
    fn rewrite_string_return_still_supported() {
        assert_eq!(
            rewrite_url(
                r#"return "https://elsewhere.test/";"#,
                "https://example.com/"
            ),
            "https://elsewhere.test/"
        );
    }

    #[test]
    fn matcher_can_read_searchparams_get() {
        let js = format!(
            r#"{}{}
globalThis.__SUPERSURFER_CONFIG__ = {{
  defaultBrowser: "chrome",
  handlers: [
    {{ match: (u) => u.searchParams.get("forta") === "firefox", browser: "firefox" }},
  ],
}};"#,
            ScriptRuntime::helpers_prelude(),
            ""
        );
        let rt = ScriptRuntime::from_js(&js).unwrap();
        let url = Url::parse("https://example.com/?forta=firefox").unwrap();
        let ctx = RouteContext::default();
        let (target, _) = rt.route(&url, &ctx).unwrap();
        assert_eq!(target.unwrap().name.as_deref(), Some("firefox"));
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
    fn invalid_glob_pattern_does_not_panic() {
        // Malformed user-config patterns must degrade to "no match", never panic.
        assert!(!glob_match("[invalid", "example.com/path"));
        assert!(!glob_match("a[b-", "example.com/path"));
        // A valid (if unusual) pattern must still not panic.
        let _ = glob_match("***", "example.com/path");
    }

    #[test]
    fn process_running_can_drive_browser_selection() {
        crate::process::replace_snapshot_for_tests(std::collections::HashSet::from([
            "msedge".to_string()
        ]));
        let js = format!(
            r#"{}{}
globalThis.__SUPERSURFER_CONFIG__ = {{
  defaultBrowser: "brave",
  handlers: [
    {{
      match: domain("github.com"),
      browser: (url) => processRunning("edge") ? "edge" : "brave",
    }},
  ],
}};"#,
            ScriptRuntime::helpers_prelude(),
            ""
        );
        let rt = ScriptRuntime::from_js(&js).unwrap();
        let url = Url::parse("https://github.com/org/repo").unwrap();
        let ctx = RouteContext::default();
        let (target, _) = rt.route(&url, &ctx).unwrap();
        assert_eq!(target.unwrap().name.as_deref(), Some("edge"));
    }

    #[test]
    fn malformed_glob_in_config_falls_back_to_default() {
        let js = format!(
            r#"{}{}
globalThis.__SUPERSURFER_CONFIG__ = {{
  defaultBrowser: "chrome",
  handlers: [{{ match: glob("[invalid"), browser: "firefox" }}],
}};"#,
            ScriptRuntime::helpers_prelude(),
            ""
        );
        let rt = ScriptRuntime::from_js(&js).unwrap();
        let url = Url::parse("https://example.com").unwrap();
        let ctx = RouteContext::default();
        let (target, _) = rt.route(&url, &ctx).unwrap();
        assert!(target.is_none(), "invalid glob should not match");
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

    fn runtime_with_url_cleaning(value: &str) -> ScriptRuntime {
        let js = format!(
            r#"{}{}
globalThis.__SUPERSURFER_CONFIG__ = {{
  defaultBrowser: "chrome",
  handlers: [],
  urlCleaning: {value},
}};"#,
            ScriptRuntime::helpers_prelude(),
            ""
        );
        ScriptRuntime::from_js(&js).unwrap()
    }

    #[test]
    fn url_cleaning_defaults_to_default_when_absent() {
        let js = format!(
            r#"{}{}
globalThis.__SUPERSURFER_CONFIG__ = {{ defaultBrowser: "chrome", handlers: [] }};"#,
            ScriptRuntime::helpers_prelude(),
            ""
        );
        let rt = ScriptRuntime::from_js(&js).unwrap();
        assert_eq!(rt.url_cleaning_mode().unwrap(), "default");
    }

    #[test]
    fn url_cleaning_off_is_respected() {
        let rt = runtime_with_url_cleaning("\"off\"");
        assert_eq!(rt.url_cleaning_mode().unwrap(), "off");
    }

    #[test]
    fn url_cleaning_default_is_respected() {
        let rt = runtime_with_url_cleaning("\"default\"");
        assert_eq!(rt.url_cleaning_mode().unwrap(), "default");
    }

    #[test]
    fn url_cleaning_unsupported_value_falls_back_to_default() {
        // Custom rule arrays are typed but not yet implemented; the runtime
        // must not silently honor an unsupported value, and must not panic.
        let rt = runtime_with_url_cleaning("\"aggressive\"");
        assert_eq!(rt.url_cleaning_mode().unwrap(), "default");
    }

    #[test]
    fn url_cleaning_custom_rule_array_falls_back_to_default() {
        // The motivating case: a custom-rule array is typed but unimplemented.
        // A non-string value must not slip through as a silent "default" via a
        // failed String coercion -- it must be detected and fall back.
        let rt = runtime_with_url_cleaning("[{ host: \"example.com\" }]");
        assert_eq!(rt.url_cleaning_mode().unwrap(), "default");
    }
}
