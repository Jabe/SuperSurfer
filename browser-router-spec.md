# Browser Router — Product & Technical Spec (v1)

Working title: **Router** (final name TBD)
Status: Draft 1 — consolidated from design discussions
Targets: Windows + macOS (v1), Linux (v1.5)

---

## 1. Problem & Goal

Power users with multiple browsers/profiles maintain two diverging configs today: Finicky (macOS) and BrowserTamer (Windows). Goal: **one engine, one config file, identical behavior on both platforms.**

The tool registers itself as the OS default browser, intercepts every URL-open event, evaluates a user-defined routing script, and forwards the URL to the resolved browser/profile.

## 2. Non-Goals (v1)

- No GUI / settings app. Config is a plain-text file; minimal native dialogs only where unavoidable (first-run, error surface).
- No declarative config dialect. TypeScript/JS is the only config language.
- No Finicky drop-in compatibility. API-familiar, not API-identical.
- No Linux support (deferred to v1.5).
- No script-accessible network or filesystem (sandbox fully locked).
- No resident daemon / tray icon.

## 3. Architecture

```
OS URL event ──▶ Router binary (Rust)
                  ├─ Config loader (TS → transpiled → QuickJS)
                  ├─ URL pre-processor (unwrap + tracker-strip ruleset)
                  ├─ decide(url, context) → RoutingResult
                  ├─ Browser resolver (abstract name → platform launch)
                  └─ Launcher (spawn browser, exit)
```

### 3.1 Engine
- **Rust** core, single static binary per platform.
- **QuickJS** via `rquickjs` as the script runtime. Rationale: tiny footprint, fast cold start, easy to sandbox, no JIT attack surface. (Fallback option if TS-transpile friction is too high: embed `swc` or pre-transpile via bundled `esbuild`-class transpiler in Rust — decision in §7.)
- **Process model: on-demand.** No daemon. The OS launches the binary per URL event; binary evaluates config, launches target browser, exits. Cold start budget: **< 50 ms** end-to-end (excluding browser launch itself).

### 3.2 Platform integration
- **macOS:** App bundle registered as handler for `http`/`https` (and optionally `mailto` later). `NSAppleEventManager` / `application(_:open:)` receives URLs. Opener app detected via `NSWorkspace` frontmost / launching application where available.
- **Windows:** Registered as a browser via `RegisterApplication` + `StartMenuInternet` registry keys, capturing the `http`/`https` URL associations. Invoked with URL as CLI arg. Opener detection: parent process inspection (best effort; documented as unreliable on Windows).

## 4. Config API

One file, e.g. `~/.config/router/config.ts` (macOS) / `%APPDATA%\router\config.ts` (Windows). Synced via any file-sync service — **the same file must work unmodified on both platforms.**

### 4.1 Entry point

```ts
export default {
  defaultBrowser: "firefox",
  handlers: [
    { match: domain("github.com"), browser: "chrome:work" },
    { match: [host("meet.google.com"), suffix(".zoom.us")], browser: "chrome:work" },
    { match: (url, ctx) => ctx.opener?.name === "Slack", browser: "firefox" },
  ],
  rewrite: [
    // optional URL transforms, applied before matching
    { match: glob("*.example.com/track/*"), url: (u) => u.searchParams.delete("ref") },
  ],
} satisfies RouterConfig;
```

### 4.2 Matcher helpers (first-class, no raw regex required)

| Helper | Semantics |
|---|---|
| `host("a.b.com")` | exact hostname match |
| `domain("b.com")` | registrable domain match (includes subdomains, PSL-aware) |
| `suffix(".b.com")` | hostname suffix match |
| `glob("*.b.com/path/*")` | glob over host+path |
| `path("/foo/*")` | path-only glob |
| `regex(/…/)` | escape hatch, available but discouraged in docs |

Matchers compose: arrays = OR, `all(...)` = AND, `not(...)` = negation. Custom predicate functions `(url, ctx) => boolean` always allowed.

### 4.3 Context object

```ts
interface Context {
  opener?: { name: string; bundleId?: string; path?: string }; // best effort
  platform: "macos" | "windows" | "linux";
  modifiers: { shift: boolean; alt: boolean; ctrl: boolean; cmd: boolean }; // where capturable
}
```

`platform` exists for edge cases but docs steer users away from platform branching — abstract browser names should make it unnecessary.

### 4.4 Routing result / browser targets

**Abstract browser names with platform-specific resolution** (settled decision):

- Plain names: `"firefox"`, `"chrome"`, `"edge"`, `"safari"`, `"brave"`, `"arc"`, …
- Profiles: `"chrome:work"`, `"firefox:Private"` — `name:profile` syntax.
- Modes: `{ browser: "firefox", private: true }` object form for incognito/private windows and future flags (e.g. `newWindow`).
- Escape hatch: `{ app: "/Applications/Foo.app" }` / `{ exe: "C:\\...\\foo.exe" }` — explicitly platform-specific, documented as sync-breaking.

**Resolution:** Router ships a built-in registry mapping abstract names → bundle IDs (macOS) / install locations & registry lookups (Windows), including profile-launch argument conventions per browser (`--profile-directory=` for Chromium, `-P` for Firefox). Auto-discovery at runtime; `router doctor` CLI lists detected browsers/profiles and their abstract names.

Unresolvable target at runtime → fall back to `defaultBrowser` + native error notification (never silently drop a URL).

## 5. Sandbox

- Scripts run in QuickJS with **no** `fetch`, no fs, no process, no timers beyond eval budget.
- Hard eval timeout (e.g. 250 ms) → fallback to `defaultBrowser` + error notification.
- Config syntax/runtime errors: same fallback semantics. The URL must always open somewhere.
- Deterministic API surface only: `URL`, matcher helpers, `console.log` (routed to log file for debugging).

## 6. Bundled URL-unwrapping / tracker-stripping ruleset

- Ships **inside the binary** (compiled-in default rules): unwrap `outlook.com/safelinks`, Google `url?q=`, common redirectors; strip `utm_*`, `fbclid`, `gclid`, etc.
- Applied **before** user `rewrite` and `handlers`, so matching sees the real destination.
- User-overridable: `urlCleaning: "off" | "default" | [custom rules…]` in config; user rules can extend or replace defaults.
- **Updates:** rules update with app releases. Additionally `router update-rules` CLI pulls a **signed** ruleset from the project's GitHub releases — runs outside the script sandbox, requires explicit user invocation. Scripts themselves remain 100% offline; the sandbox guarantee is never weakened.

## 7. Config loading & TS support

- Accept `.ts` and `.js`. TS support via embedded transpile (type-strip only, no type-checking at runtime).
- Ship a `router-types` npm package (or bundled `.d.ts` emitted by `router init`) so editors give full IntelliSense; runtime ignores types.
- Cache transpiled output keyed by config mtime+hash to keep cold start under budget.
- **Open implementation choice:** embed SWC (Rust-native, heavier binary) vs. minimal custom type-stripper. Default lean: SWC, measure binary size impact.

## 8. CLI

| Command | Purpose |
|---|---|
| `router init` | scaffold config + types, register as default browser (guided) |
| `router doctor` | list detected browsers/profiles, validate config, show registration status |
| `router test <url> [--opener X]` | dry-run: print routing decision without opening |
| `router update-rules` | fetch signed default ruleset update |
| `router logs` | tail decision log |

`router test` is the primary debugging loop — fast iteration without clicking links.

## 9. Migration

- **From Finicky:** migration guide mapping common patterns; optional one-shot `router migrate --finicky ~/.finicky.js` best-effort converter (emits TODO comments where semantics differ). No runtime compat layer.
- **From BrowserTamer:** guide only (rule model is simple enough that manual migration is minutes).

## 10. Roadmap

| Version | Scope |
|---|---|
| v1.0 | Windows + macOS, full spec above |
| v1.1 | `mailto:` handling, modifier-key routing polish |
| v1.5 | Linux (XDG desktop entry, `x-scheme-handler/http(s)`), opener detection via parent PID |
| v2.x | optional picker UI on no-match, per-rule logging/stats |

## 11. Open items (deliberately small)

1. Final name + binary name.
2. SWC vs. custom type-stripper (binary size vs. effort) — §7.
3. Modifier-key capture feasibility per platform (may degrade to best-effort, documented).
4. Signing/notarization pipeline (Apple notarization required for default-browser registration UX; Windows SmartScreen reputation).
