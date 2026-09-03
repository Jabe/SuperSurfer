# SuperSurfer

**One config. Every browser. macOS · Windows · Linux.**

[![Release](https://img.shields.io/github/v/release/Jabe/SuperSurfer)](https://github.com/Jabe/SuperSurfer/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/Jabe/SuperSurfer/ci.yml?branch=main)](https://github.com/Jabe/SuperSurfer/actions)
[![License: MIT](https://img.shields.io/github/license/Jabe/SuperSurfer)](LICENSE)

SuperSurfer registers as your OS default browser, intercepts every link open, evaluates a JavaScript routing config, and forwards the URL to the right browser/profile.

A [Finicky](https://github.com/johnste/finicky)-style router that also runs on Windows and Linux, with built-in safelink unwrapping and tracker stripping.

**Manual:** [docs/manual.md](docs/manual.md) (also opened in your browser on first run)

| | SuperSurfer | Finicky |
|---|---|---|
| Platforms | macOS, Windows, Linux | macOS |
| Config | JavaScript (`config.js`) | JavaScript / TypeScript |
| URL cleaning | Built-in safelink unwrap + tracker strip | Rewrite rules you write |
| Browser profiles | `chrome:work`, `{ name, profile }` | `{ name, profile }` |
| License | MIT | MIT |

## Install

Binaries are on the [Releases](https://github.com/Jabe/SuperSurfer/releases) page. Direct links always point at the latest:

| Platform | Download |
|---|---|
| Windows x86_64 | [`supersurfer.exe`](https://github.com/Jabe/SuperSurfer/releases/latest/download/supersurfer.exe) |
| macOS Apple Silicon | [`SuperSurfer.app.zip`](https://github.com/Jabe/SuperSurfer/releases/latest/download/SuperSurfer.app.zip) |
| Linux x86_64 | [`supersurfer-linux-x86_64.tar.gz`](https://github.com/Jabe/SuperSurfer/releases/latest/download/supersurfer-linux-x86_64.tar.gz) |
| Linux aarch64 | [`supersurfer-linux-aarch64.tar.gz`](https://github.com/Jabe/SuperSurfer/releases/latest/download/supersurfer-linux-aarch64.tar.gz) |

Then register SuperSurfer as the default browser and try a dry-run:

**macOS** — unzip into `/Applications`, then:

```bash
/Applications/SuperSurfer.app/Contents/MacOS/SuperSurfer register
/Applications/SuperSurfer.app/Contents/MacOS/SuperSurfer test https://github.com/org/repo
```

**Windows:**

```powershell
.\supersurfer.exe init --register
.\supersurfer.exe test https://github.com/org/repo
```

Set SuperSurfer as default under **Settings → Apps → Default apps**.

**Linux:**

```bash
tar -xzf supersurfer-linux-x86_64.tar.gz   # or aarch64
cd linux && ./install.sh                   # linux-aarch64 on ARM
supersurfer register
supersurfer test https://github.com/org/repo
```

First run (no arguments) creates `config.js` and `supersurfer.d.ts`, then opens the [setup guide](docs/manual.md). URL routing (`supersurfer https://…`) also bootstraps config silently when needed.

Config lives at:

- macOS: `~/Library/Application Support/SuperSurfer/config.js`
- Windows: `%APPDATA%\SuperSurfer\config.js`
- Linux: `~/.config/SuperSurfer/config.js`

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
    { match: [host("meet.google.com"), suffix(".zoom.us")], browser: "chrome:work" },
    { match: (url, ctx) => ctx.opener?.name === "Slack", browser: "firefox" },
  ],
};
```

`processRunning("edge")` checks whether a browser (by id, display name, or process name) is running — snapshot on first call per route, then cached for that evaluation.

## From Finicky

There is no built-in migrate command. SuperSurfer already supports most Finicky config patterns (`{ name, profile }` browser targets, dynamic `browser` handlers, `rewrite` rules). Copy your `~/.finicky.js` into `config.js`, add a `/** @type {import('./supersurfer').RouterConfig} */` comment above `export default`, then adjust:

- `finicky.matchHostnames([...])` → a local `matchHostnames()` helper, or `host` / `suffix` / `regex` matchers
- `finicky.opener` → `ctx.opener`
- custom `rewrite` + built-in URL cleaning may overlap — the built-in rules cover Outlook/Teams safelinks, Google, Slack, LinkedIn and more, so the equivalent `rewrite` rules can usually just go

An LLM plus `supersurfer.d.ts` (written by `supersurfer init`) is the intended migration path. Validate with `supersurfer doctor` and `supersurfer test <url>`.

## URL cleaning

Built-in rules unwrap redirect wrappers (Outlook/Teams safelinks, Azure Communication Services, Google, Slack, Facebook, LinkedIn, Trend Micro, Barracuda, Sophos) and strip tracking parameters (`utm_*`, `fbclid`, `gclid`, …). `urlCleaning` controls how far that reaches:

| Mode | Handlers match on | Browser opens |
|---|---|---|
| `"route"` *(default)* | the decoded destination | the URL as it arrived |
| `"direct"` | the decoded destination | the decoded destination |
| `"off"` | the raw URL | the raw URL |

`route` is the default because it separates two concerns that are easy to conflate. Your handlers get to see that a mailed safelink really points at `github.com` and route it accordingly — while the wrapper still reaches the browser, so link scanning, revocation and click reporting your organisation may depend on keep working.

Pick `direct` to skip the redirector entirely: one HTTP round-trip less and no click reported, at the cost of whatever checks that redirector performs. Pick `off` only if matchers should see raw safelinks, e.g. because you route on the wrapper itself.

A `rewrite` rule or a `resolve` preflight always reaches the browser — both are deliberate moves, not something cleaning should undo. A *resolved* destination is cleaned first, though: whatever sits at the end of a redirect chain is unknown and may be a wrapper itself, so it gets the same treatment as the original input. `rewrite` results are left alone, since those are yours. `file:` URLs are never touched in any mode.

Worth knowing under `route`: allowing a host with `supersurfer resolve allow` gives up wrapper preservation **for that host**. The HEAD request has already been made and the destination is known, so re-opening the redirector would only repeat work you deliberately paid for. If you chose `route` specifically to keep a scanner in the loop, keep that host off the resolve allowlist.

`supersurfer test <url>` prints `routed:` and `opens:` separately, which is the quickest way to see a mode in action.

## CLI

| Command | Purpose |
|---|---|
| `supersurfer init` | Scaffold config + types |
| `supersurfer register` | Register as default browser |
| `supersurfer doctor` | List browsers, validate config |
| `supersurfer test <url>` | Dry-run routing decision |
| `supersurfer logs` | Tail decision log |
| `supersurfer update-rules` | Fetch signed URL-cleaning rules (planned) |

When registered as the default browser, the OS invokes the packaged app with the URL (macOS: `SuperSurfer.app`; Windows: `supersurfer.exe "%1"`; Linux: `supersurfer %u` via `supersurfer.desktop`).

## Build from source

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
./target/release/supersurfer          # first run: scaffolds config + opens manual
./target/release/supersurfer doctor
./target/release/supersurfer test https://github.com/org/repo
./target/release/supersurfer register
```

Legacy explicit init:

```bash
./target/release/supersurfer init
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

Download `supersurfer.exe` from [Releases](https://github.com/Jabe/SuperSurfer/releases), or build locally.

On Windows, or cross-compile from macOS/Linux (`mise` installs `zig`; first run may install `cargo-zigbuild`):

```bash
mise run package-windows
```

On Windows:

```powershell
.\dist\supersurfer.exe init --register
.\dist\supersurfer.exe test https://github.com/foo
```

`init --register` writes `StartMenuInternet` registry entries so SuperSurfer appears in **Settings → Apps → Default apps**. Search for SuperSurfer, open it, then click **Set default** (or assign HTTP, HTTPS, `.htm`, and `.html` individually). The old “Web browser” picker was removed in Windows 11.

### Linux — `supersurfer`

Download the tarball for your arch from [Releases](https://github.com/Jabe/SuperSurfer/releases). Builds are dynamically linked glibc binaries for **x86_64** and **aarch64**. Release artifacts are built on Ubuntu 22.04 (glibc 2.35) as the minimum supported baseline; newer distros work too.

**x86_64 (Intel/AMD):**
```bash
tar -xzf supersurfer-linux-x86_64.tar.gz
cd linux
./install.sh
```

**aarch64 (ARM, e.g. Raspberry Pi, ARM laptops):**
```bash
tar -xzf supersurfer-linux-aarch64.tar.gz
cd linux-aarch64
./install.sh
```

From source (native or cross-compile):
```bash
mise run package-linux          # native x86_64 on Linux
mise run package-linux-arm      # cross-compile aarch64 (needs zig)
```

Then:
```bash
supersurfer init
supersurfer register         # sets default via xdg-settings / xdg-mime
supersurfer doctor
```

`register` installs `supersurfer.desktop` into `~/.local/share/applications/` and runs `xdg-settings set default-web-browser supersurfer.desktop` (with an `xdg-mime` fallback for `http`/`https`). Native browsers are discovered via `.desktop` files in the XDG application directories; Flatpak/Snap browsers are not yet supported.

## Architecture

```
OS URL event → SuperSurfer (Rust)
                ├─ Config loader (JS → QuickJS)
                ├─ URL pre-processor (unwrap + tracker strip)
                ├─ handlers / rewrite evaluation
                ├─ Browser resolver (abstract name → platform launch)
                └─ Launcher (spawn browser, exit)
```

## Status

v0.1.x is usable as a default browser on macOS, Windows, and Linux:

- Rust core with QuickJS sandboxed config runtime
- JavaScript config with JSDoc types
- Matcher helpers (`host`, `domain`, `suffix`, `glob`, `path`, `regex`, `all`, `not`, `processRunning`)
- Built-in URL cleaning with `route` / `direct` / `off` modes (safelink unwrapping, UTM stripping)
- macOS `SuperSurfer.app` bundle + Launch Services / duti registration
- Windows `supersurfer.exe` + registry browser registration
- Linux `supersurfer` binary + `.desktop` / xdg-settings registration
- macOS, Windows, and Linux browser discovery + launch
- CLI: `init`, `doctor`, `test`, `logs`

**Not yet:** signed/notarized distribution, signed rules updates. macOS Gatekeeper will warn on first open until the app is notarized — right-click → Open, or `xattr -cr /Applications/SuperSurfer.app`.

## License

MIT
