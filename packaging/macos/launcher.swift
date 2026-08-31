import Cocoa
import Foundation

func routerBinary() -> URL {
    Bundle.main.executableURL!
        .deletingLastPathComponent()
        .appendingPathComponent("supersurfer-bin")
}

func isRoutableInput(_ value: String) -> Bool {
    if value.hasPrefix("http://") || value.hasPrefix("https://") || value.hasPrefix("file://") {
        return true
    }
    if value.hasPrefix("/") || value.hasPrefix("~/") {
        return true
    }
    return false
}

// Identify the app the user clicked the link in. SuperSurfer runs as an
// `.accessory` and never activates itself, so the frontmost application at the
// moment the open event arrives is the originating app (e.g. Slack, Mail).
// This is passed to the router so `ctx.opener` works — parent-process detection
// alone would only ever report SuperSurfer/launchd.
func openerArgs() -> [String] {
    guard let app = NSWorkspace.shared.frontmostApplication else { return [] }
    if let bundleID = app.bundleIdentifier, bundleID == Bundle.main.bundleIdentifier {
        return []
    }
    var args: [String] = []
    if let name = app.localizedName {
        args += ["--opener-name", name]
    }
    if let bundleID = app.bundleIdentifier {
        args += ["--opener-bundle", bundleID]
    }
    if let path = app.bundleURL?.path {
        args += ["--opener-path", path]
    }
    return args
}

func runProcess(arguments: [String]) {
    let process = Process()
    process.executableURL = routerBinary()
    process.arguments = arguments
    process.standardOutput = FileHandle.standardOutput
    process.standardError = FileHandle.standardError
    do {
        try process.run()
        process.waitUntilExit()
    } catch {
        FileHandle.standardError.write(
            Data("failed to run supersurfer-bin: \(error)\n".utf8)
        )
    }
}

func runRouter(with urls: [String], opener: [String]) {
    for url in urls {
        runProcess(arguments: opener + [url])
    }
}

func runSupersurfer(args: [String]) {
    runProcess(arguments: args)
}

func cliArgs() -> [String] {
    Array(ProcessInfo.processInfo.arguments.dropFirst())
        .filter { !$0.hasPrefix("-psn_") }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var finished = false
    private var inflight = 0

    func applicationDidFinishLaunching(_ notification: Notification) {
        let args = cliArgs()
        let urls = args.filter(isRoutableInput)
        let commands = args.filter { !isRoutableInput($0) }

        if !urls.isEmpty {
            finished = true
            handleUrlsThenQuit(urls)
            return
        }

        if !commands.isEmpty {
            finished = true
            runSupersurfer(args: commands)
            NSApp.terminate(nil)
            return
        }

        // Wait briefly for a default-browser URL event, then run first-run bootstrap.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
            guard let self, !self.finished else { return }
            self.finished = true
            runSupersurfer(args: [])
            NSApp.terminate(nil)
        }
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        finished = true
        let routable = urls.map(\.absoluteString).filter(isRoutableInput)
        if routable.isEmpty {
            quitIfIdle()
            return
        }
        handleUrlsThenQuit(routable)
    }

    /// Route URLs off the main thread. Waiting on the router here used to pin
    /// the Cocoa run loop: one hung `supersurfer-bin` (e.g. waiting for Edge)
    /// made every later link click a no-op, with nothing written to the log.
    private func handleUrlsThenQuit(_ urls: [String]) {
        let opener = openerArgs()
        inflight += 1
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            runRouter(with: urls, opener: opener)
            DispatchQueue.main.async {
                self?.inflight -= 1
                self?.quitIfIdle()
            }
        }
    }

    private func quitIfIdle() {
        if inflight == 0 {
            NSApp.terminate(nil)
        }
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
