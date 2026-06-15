import AppKit
import Foundation

guard CommandLine.arguments.count >= 2 else {
    fputs("usage: set-default <path-to-SuperSurfer.app>\n", stderr)
    exit(1)
}

let app = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let workspace = NSWorkspace.shared
let group = DispatchGroup()
var lastError: Error?

for scheme in ["https", "http"] {
    group.enter()
    workspace.setDefaultApplication(at: app, toOpenURLsWithScheme: scheme) { error in
        if let error {
            lastError = error
        }
        group.leave()
    }
}

group.wait()

if let lastError {
    fputs("failed to set default browser: \(lastError)\n", stderr)
    exit(1)
}

print("Set SuperSurfer as default browser for http and https.")
