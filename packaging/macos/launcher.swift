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

func runRouter(with urls: [String]) {
    for url in urls {
        let process = Process()
        process.executableURL = routerBinary()
        process.arguments = [url]
        process.standardOutput = FileHandle.standardOutput
        process.standardError = FileHandle.standardError
        try? process.run()
        process.waitUntilExit()
    }
}

func runSupersurfer(args: [String]) {
    let process = Process()
    process.executableURL = routerBinary()
    process.arguments = args
    process.standardOutput = FileHandle.standardOutput
    process.standardError = FileHandle.standardError
    try? process.run()
    process.waitUntilExit()
}

func cliArgs() -> [String] {
    Array(ProcessInfo.processInfo.arguments.dropFirst())
        .filter { !$0.hasPrefix("-psn_") }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var finished = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        let args = cliArgs()
        let urls = args.filter(isRoutableInput)
        let commands = args.filter { !isRoutableInput($0) }

        if !urls.isEmpty {
            finished = true
            runRouter(with: urls)
            NSApp.terminate(nil)
            return
        }

        if !commands.is_empty {
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
        if !routable.isEmpty {
            runRouter(with: routable)
        }
        NSApp.terminate(nil)
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
