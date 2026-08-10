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

### macOS

```bash
mise run package-macos
cp -R dist/SuperSurfer.app /Applications/
/Applications/SuperSurfer.app/Contents/MacOS/SuperSurfer register
```

### Windows

Download `supersurfer.exe` from CI artifacts, then:

```powershell
.\supersurfer.exe register
```

Set SuperSurfer as default under **Settings → Apps → Default apps**.

### Linux

```bash
tar -xzf supersurfer-linux-aarch64.tar.gz   # or x86_64
cd linux-aarch64 && ./install.sh
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
