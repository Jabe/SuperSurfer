# SuperSurfer manual

SuperSurfer registers as your default browser, intercepts link opens, runs your TypeScript routing config, and forwards each URL to the right browser and profile.

## First run

The first time you start SuperSurfer it creates:

- `config.ts` — your routing rules
- `supersurfer.d.ts` — TypeScript types for editors

Config locations:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/SuperSurfer/config.ts` |
| Windows | `%APPDATA%\SuperSurfer\config.ts` |
| Linux | `~/.config/SuperSurfer/config.ts` |

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

```ts
import type { RouterConfig } from "./supersurfer";

export default {
  defaultBrowser: "chrome",
  urlCleaning: "default",
  handlers: [
    { match: domain("github.com"), browser: "chrome" },
    { match: host("meet.google.com"), browser: "chrome:work" },
    { match: (url, ctx) => ctx.opener?.name === "Slack", browser: "firefox" },
  ],
} satisfies RouterConfig;
```

Matcher helpers: `host`, `domain`, `suffix`, `glob`, `path`, `regex`, `all`, `not`.

Browser targets: `"chrome"`, `"firefox:Profile Name"`, `{ name: "Microsoft Edge", profile: "Work" }`.

## From Finicky

Copy your `~/.finicky.js` into `config.ts`, add the `import type` line and `satisfies RouterConfig`. Replace `finicky.matchHostnames([...])` with a local helper or built-in matchers. Use an LLM plus `supersurfer.d.ts` for one-shot migration. Validate with `supersurfer doctor` and `supersurfer test <url>`.

## Troubleshooting

**No browsers in `doctor` (Linux)** — Snap browsers use names like `firefox_firefox.desktop`. Use a current SuperSurfer build; native `.deb` installs are detected most reliably.

**Wrong architecture (Linux)** — ARM machines need `supersurfer-linux-aarch64.tar.gz`, not the x86_64 build.

**Routing falls back to default** — run `supersurfer test <url>` and fix config errors shown on stderr.

Repository: [github.com/Jabe/SuperSurfer](https://github.com/Jabe/SuperSurfer)
