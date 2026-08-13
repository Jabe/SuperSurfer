# SuperSurfer manual

SuperSurfer registers as your default browser, intercepts link opens, runs your JavaScript routing config, and forwards each URL to the right browser and profile.

## First run

The first time you start SuperSurfer it creates:

- `config.js` — your routing rules
- `supersurfer.d.ts` — type definitions for editor autocomplete (via JSDoc)

Config locations:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/SuperSurfer/config.js` |
| Windows | `%APPDATA%\SuperSurfer\config.js` |
| Linux | `~/.config/SuperSurfer/config.js` |

## Install per platform

Download the matching artifact from [Releases](https://github.com/Jabe/SuperSurfer/releases).

### macOS

Unzip `SuperSurfer.app.zip` into `/Applications`, then:

```bash
/Applications/SuperSurfer.app/Contents/MacOS/SuperSurfer register
```

From source: `mise run package-macos` and copy `dist/SuperSurfer.app` into `/Applications`.

### Windows

Download `supersurfer.exe`, then:

```powershell
.\supersurfer.exe init --register
```

Set SuperSurfer as default under **Settings → Apps → Default apps**.

### Linux

```bash
tar -xzf supersurfer-linux-x86_64.tar.gz   # or aarch64
cd linux && ./install.sh                   # linux-aarch64 on ARM
supersurfer register
```

## Everyday commands

```bash
supersurfer doctor              # browsers, config, registration
supersurfer test https://example.com
supersurfer test https://example.com --open
supersurfer register            # (re)register as default browser
supersurfer logs
```

## Example config

```js
/** @type {import('./supersurfer').RouterConfig} */
export default {
  defaultBrowser: "chrome",
  urlCleaning: "route",
  handlers: [
    {
      match: domain("github.com"),
      browser: (url) => (processRunning("edge") ? "edge" : "chrome"),
    },
    { match: host("meet.google.com"), browser: "chrome:work" },
    { match: (url, ctx) => ctx.opener?.name === "Slack", browser: "firefox" },
  ],
};
```

Matcher helpers: `host`, `domain`, `suffix`, `glob`, `path`, `regex`, `all`, `not`, `processRunning`.

`urlCleaning` decides how far built-in safelink unwrapping reaches: `"route"` (default) decodes for the matching decision but hands the browser the URL as it arrived, so the wrapper's link scanning stays intact; `"direct"` opens the decoded destination and skips the redirector; `"off"` disables decoding entirely. `rewrite` and `resolve` results always reach the browser regardless. `supersurfer test <url>` prints `routed:` and `opens:` separately.

`processRunning(name)` returns whether a browser is running (e.g. `"edge"`, `"Microsoft Edge"`). The process list is snapshotted on the first call in each route, then reused for that link.

Browser targets: `"chrome"`, `"firefox:Profile Name"`, `{ name: "Microsoft Edge", profile: "Work" }`.

## From Finicky

Copy your `~/.finicky.js` into `config.js`, add a `/** @type {import('./supersurfer').RouterConfig} */` comment above `export default`. Replace `finicky.matchHostnames([...])` with a local helper or built-in matchers. Use an LLM plus `supersurfer.d.ts` for one-shot migration. Validate with `supersurfer doctor` and `supersurfer test <url>`.

## Troubleshooting

**No browsers in `doctor` (Linux)** — Snap browsers use names like `firefox_firefox.desktop`. Use a current SuperSurfer build; native `.deb` installs are detected most reliably.

**Wrong architecture (Linux)** — ARM machines need `supersurfer-linux-aarch64.tar.gz`, not the x86_64 build.

**Routing falls back to default** — run `supersurfer test <url>` and fix config errors shown on stderr.

Repository: [github.com/Jabe/SuperSurfer](https://github.com/Jabe/SuperSurfer)
