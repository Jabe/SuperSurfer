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

## CLI

| Command | Purpose |
|---|---|
| `supersurfer init` | Scaffold config + types |
| `supersurfer doctor` | List browsers, validate config |
| `supersurfer test <url>` | Dry-run routing decision |
| `supersurfer logs` | Tail decision log |
| `supersurfer update-rules` | Fetch signed URL-cleaning rules (planned) |

When registered as the default browser, the OS invokes `supersurfer <url>` directly.

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

This is an initial implementation of the [browser router spec](./browser-router-spec.md):

- Rust core with QuickJS sandboxed config runtime
- TypeScript config via lightweight type-stripping + cache
- Matcher helpers (`host`, `domain`, `suffix`, `glob`, `path`, `regex`, `all`, `not`)
- Built-in URL cleaning (Outlook safelinks, Google redirects, UTM stripping)
- macOS browser discovery + launch
- CLI: `init`, `doctor`, `test`, `logs`

**Not yet implemented:** default-browser auto-registration app bundle, Windows browser discovery, signed rules updates, Finicky migration.

## License

MIT
