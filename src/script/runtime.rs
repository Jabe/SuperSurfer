use crate::context::Context as RouteContext;
use anyhow::{anyhow, Context as _, Result};
use globset::{Glob, GlobMatcher};
use rquickjs::{Array, CatchResultExt, Context, Function, Object, Runtime, Value};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::cell::RefCell;
use std::time::{Duration, Instant};
use url::Url;

const EVAL_TIMEOUT: Duration = Duration::from_millis(250);

static GLOB_MATCHERS: LazyLock<Mutex<HashMap<String, GlobMatcher>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct ScriptRuntime {
    _runtime: Runtime,
    ctx: Context,
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
        let runtime = Runtime::new().context("failed to create QuickJS runtime")?;
        let ctx = Context::full(&runtime).context("failed to create QuickJS context")?;

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

    pub fn route(
        &self,
        url: &Url,
        context: &RouteContext,
    ) -> Result<(Option<BrowserTarget>, Url)> {
        let started = Instant::now();
        let working = RefCell::new(url.clone());
        let result = self.route_inner(&working, context);
        if started.elapsed() > EVAL_TIMEOUT {
            anyhow::bail!("routing eval exceeded {} ms budget", EVAL_TIMEOUT.as_millis());
        }
        let target = result?;
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
        let id = map_browser_name(&display_name)?;
        let spec = match profile {
            Some(profile) => format!("{id}:{profile}"),
            None => id,
        };
        return Ok(BrowserTarget {
            name: Some(spec),
            private: false,
            app: None,
            exe: None,
        });
    }

    let browser: Option<String> = obj.get("browser").ok();
    let private: bool = obj.get("private").unwrap_or(false);
    let app: Option<String> = obj.get("app").ok();
    let exe: Option<String> = obj.get("exe").ok();

    Ok(BrowserTarget {
        name: browser,
        private,
        app,
        exe,
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
        url.query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default(),
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
