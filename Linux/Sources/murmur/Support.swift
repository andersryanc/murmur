import Foundation
import Glibc

/// Where Murmur keeps things, following the XDG base directories.
enum Paths {
    /// `~/.config/murmur`: config.json and dictionary.json.
    static var config: URL { base("XDG_CONFIG_HOME", ".config") }
    /// `~/.local/share/murmur`: the speech model.
    static var data: URL { base("XDG_DATA_HOME", ".local/share") }
    /// `~/.local/state/murmur`: murmur.log.
    static var state: URL { base("XDG_STATE_HOME", ".local/state") }
    /// `$XDG_RUNTIME_DIR/murmur`: the control socket and rendered sounds. Cleared at logout.
    static var runtime: URL {
        let dir = env("XDG_RUNTIME_DIR").map { URL(fileURLWithPath: $0) }
            ?? URL(fileURLWithPath: "/tmp/murmur-\(getuid())")
        return dir.appending(path: "murmur")
    }

    static var socket: URL { runtime.appending(path: "murmur.sock") }
    static var log: URL { state.appending(path: "murmur.log") }

    static func env(_ name: String) -> String? {
        guard let value = ProcessInfo.processInfo.environment[name], !value.isEmpty else { return nil }
        return value
    }

    private static func base(_ variable: String, _ fallback: String) -> URL {
        let dir = env(variable).map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appending(path: fallback)
        return dir.appending(path: "murmur")
    }

    static func ensure(_ dir: URL) {
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }
}

/// Prints to stderr and appends to `~/.local/state/murmur/murmur.log`, since the daemon usually
/// runs from Hyprland's autostart with nowhere to print.
enum Log {
    static func info(_ message: String) {
        let line = "\(Date().formatted(.iso8601)) \(message)\n"
        FileHandle.standardError.write(Data(line.utf8))
        Paths.ensure(Paths.state)
        let url = Paths.log
        if !FileManager.default.fileExists(atPath: url.path) {
            _ = FileManager.default.createFile(atPath: url.path, contents: nil)
        }
        guard let file = try? FileHandle(forWritingTo: url) else { return }
        defer { try? file.close() }
        _ = try? file.seekToEnd()
        try? file.write(contentsOf: Data(line.utf8))
    }
}

/// Runs the desktop's command-line tools.
enum Shell {
    struct Result {
        let status: Int32
        let output: String
        var succeeded: Bool { status == 0 }
    }

    /// Runs `program` from PATH to completion. `input` is written to its stdin. Output is only
    /// captured when asked for, and must be small (it's read after the program exits); never
    /// capture from a program that leaves a child running, like wl-copy.
    ///
    /// Waits with a termination handler rather than `waitUntilExit()`, which spins the run loop
    /// and could run other daemon work in the middle of this call.
    @discardableResult
    static func run(_ program: String, _ arguments: [String] = [], input: String? = nil,
                    captureOutput: Bool = false) async -> Result {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = [program] + arguments
        let stdout = Pipe()
        let stdin = Pipe()
        process.standardOutput = captureOutput ? stdout : FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        process.standardInput = input == nil ? FileHandle.nullDevice : stdin

        let status: Int32 = await withCheckedContinuation { continuation in
            process.terminationHandler = { continuation.resume(returning: $0.terminationStatus) }
            do {
                try process.run()
            } catch {
                process.terminationHandler = nil
                continuation.resume(returning: -1)
                return
            }
            if let input {
                stdin.fileHandleForWriting.write(Data(input.utf8))
                try? stdin.fileHandleForWriting.close()
            }
        }
        let output = captureOutput ? String(decoding: stdout.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self) : ""
        return Result(status: status, output: output)
    }

    /// Starts `program` and doesn't wait for it.
    static func spawn(_ program: String, _ arguments: [String] = []) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = [program] + arguments
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        process.standardInput = FileHandle.nullDevice
        try? process.run()
    }

    /// True if `program` is on PATH.
    static func has(_ program: String) -> Bool {
        (Paths.env("PATH") ?? "/usr/bin").split(separator: ":").contains {
            FileManager.default.isExecutableFile(atPath: "\($0)/\(program)")
        }
    }
}
