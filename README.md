# SuperSurfer

**One config. Every browser. macOS + Windows.**

SuperSurfer registers as your OS default browser, intercepts every link open, evaluates a TypeScript routing script, and forwards the URL to the right browser/profile.

## Quick start

This project uses [mise](https://mise.jdx.dev/) for tool versions (Rust, rustfmt, clippy).

```bash
mise trust            # first time in this repo
mise install          # install pinned Rust toolchain
mise run build
mise run dev -- init
mise run dev -- test https://github.com/org/repo
mise run doctor
```

Or without mise tasks:

```bash
cargo build --release
./target/release/supersurfer init
./target/release/supersurfer test https://github.com/org/repo
./target/release/supersurfer doctor
```

Config lives at:

- macOS: `~/Library/Application Support/SuperSurfer/config.ts`
- Windows: `%APPDATA%\SuperSurfer\config.ts`

## Example config

```ts
import type { RouterConfig } from "./supersurfer";

export default {
  defaultBrowser: "safari",
  urlCleaning: "default",
  handlers: [
    { match: domain("github.com"), browser: "chrome" },
    { match: [host("meet.google.com"), suffix(".zoom.us")], browser: "chrome:work" },
    { match: (url, ctx) => ctx.opener?.name === "Slack", browser: "firefox" },
  ],
} satisfies RouterConfig;
```

## Packaging (default browser)

### macOS — `SuperSurfer.app`

```bash
mise run package-macos
cp -R dist/SuperSurfer.app /Applications/
mise run register
# or: /Applications/SuperSurfer.app/Contents/MacOS/SuperSurfer register
/Applications/SuperSurfer.app/Contents/MacOS/SuperSurfer test https://github.com/foo
```

Note: if your shell aliases `open` to `xdg-open`, use the full app path above — not `open -a SuperSurfer`.

The bundle contains a small Cocoa launcher (`SuperSurfer`) that receives `http`/`https` URL events and forwards them to the Rust router (`supersurfer-bin`). On macOS's case-insensitive filesystem these must be distinct names.

### Windows — `supersurfer.exe`

On a Windows machine (or cross-compile with `rustup target add x86_64-pc-windows-msvc`):

```powershell
mise run package-windows
.\dist\supersurfer.exe init --register
.\dist\supersurfer.exe test https://github.com/foo
```

`init --register` writes the `StartMenuInternet` registry entries so SuperSurfer appears under **Settings → Default apps → Web browser**.

## CLI

| Command | Purpose |
|---|---|
| `supersurfer init` | Scaffold config + types |
| `supersurfer register` | Register as default browser |
| `supersurfer doctor` | List browsers, validate config |
| `supersurfer test <url>` | Dry-run routing decision |
| `supersurfer logs` | Tail decision log |
| `supersurfer update-rules` | Fetch signed URL-cleaning rules (planned) |

When registered as the default browser, the OS invokes the packaged app with the URL (macOS: `SuperSurfer.app`; Windows: `supersurfer.exe "%1"`).

## Architecture

```
OS URL event → SuperSurfer (Rust)
                ├─ Config loader (TS → type-strip → QuickJS)
                ├─ URL pre-processor (unwrap + tracker strip)
                ├─ handlers / rewrite evaluation
                ├─ Browser resolver (abstract name → platform launch)
                └─ Launcher (spawn browser, exit)
```

## Status

This is an initial implementation of the browser router spec:

- Rust core with QuickJS sandboxed config runtime
- TypeScript config via lightweight type-stripping + cache
- Matcher helpers (`host`, `domain`, `suffix`, `glob`, `path`, `regex`, `all`, `not`)
- Built-in URL cleaning (Outlook safelinks, Google redirects, UTM stripping)
- macOS `SuperSurfer.app` bundle + Launch Services / duti registration
- Windows `supersurfer.exe` + registry browser registration
- macOS browser discovery + launch
- CLI: `init`, `doctor`, `test`, `logs`

**Not yet implemented:** Windows browser discovery, signed/notarized distribution, signed rules updates.

## License

MIT
