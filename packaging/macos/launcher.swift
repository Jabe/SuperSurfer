import Cocoa
import Foundation

func routerBinary() -> URL {
    Bundle.main.executableURL!
        .deletingLastPathComponent()
        .appendingPathComponent("supersurfer-bin")
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
        let urls = args.filter { $0.hasPrefix("http://") || $0.hasPrefix("https://") }
        let commands = args.filter { !$0.hasPrefix("http://") && !$0.hasPrefix("https://") }

        if !urls.isEmpty {
            finished = true
            runRouter(with: urls)
            NSApp.terminate(nil)
            return
        }

        if !commands.isEmpty {
            finished = true
            runSupersurfer(args: commands)
            NSApp.terminate(nil)
            return
        }

        // Default-browser mode: wait briefly for an incoming http(s) URL event.
        DispatchQueue.main.asyncAfter(deadline: .now() + 3.0) { [weak self] in
            guard let self, !self.finished else { return }
            self.finished = true
            NSApp.terminate(nil)
        }
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        finished = true
        let http = urls.map(\.absoluteString).filter {
            $0.hasPrefix("http://") || $0.hasPrefix("https://")
        }
        if !http.isEmpty {
            runRouter(with: http)
        }
        NSApp.terminate(nil)
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
