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
    /// captured when asked for; never capture from a program that leaves a child running, like
    /// wl-copy, since its output never ends.
    ///
    /// Uses posix_spawn rather than Foundation's `Process`, which on Linux learns that a program
    /// exited when a socket it passes to the program closes. wl-copy forks a child that keeps
    /// serving the clipboard and inherits that socket, so `Process` never saw wl-copy finish.
    /// Here the program gets only stdin, stdout and stderr, and is waited for with waitpid.
    @discardableResult
    static func run(_ program: String, _ arguments: [String] = [], input: String? = nil,
                    captureOutput: Bool = false) async -> Result {
        await withCheckedContinuation { continuation in
            DispatchQueue.global().async {
                continuation.resume(returning: runBlocking(program, arguments, input: input, captureOutput: captureOutput))
            }
        }
    }

    private static func runBlocking(_ program: String, _ arguments: [String], input: String?,
                                    captureOutput: Bool) -> Result {
        var stdinPipe: [Int32] = [-1, -1]
        var stdoutPipe: [Int32] = [-1, -1]
        if input != nil, pipe(&stdinPipe) != 0 { return Result(status: -1, output: "") }
        if captureOutput, pipe(&stdoutPipe) != 0 {
            for fd in stdinPipe { close(fd) }
            return Result(status: -1, output: "")
        }
        // Close-on-exec, so a program started elsewhere at the same moment (pw-record through
        // `Process`) can't inherit them and hold wl-copy's stdin open. dup2 clears the flag on
        // the copies this program gets. (Swift's Glibc doesn't import pipe2.)
        for fd in stdinPipe + stdoutPipe where fd >= 0 { _ = fcntl(fd, F_SETFD, FD_CLOEXEC) }

        var actions = posix_spawn_file_actions_t()
        posix_spawn_file_actions_init(&actions)
        defer { posix_spawn_file_actions_destroy(&actions) }
        if input != nil {
            posix_spawn_file_actions_adddup2(&actions, stdinPipe[0], 0)
        } else {
            posix_spawn_file_actions_addopen(&actions, 0, "/dev/null", O_RDONLY, 0)
        }
        if captureOutput {
            posix_spawn_file_actions_adddup2(&actions, stdoutPipe[1], 1)
        } else {
            posix_spawn_file_actions_addopen(&actions, 1, "/dev/null", O_WRONLY, 0)
        }
        posix_spawn_file_actions_addopen(&actions, 2, "/dev/null", O_WRONLY, 0)
        posix_spawn_file_actions_addclosefrom_np(&actions, 3)

        let argv = ([program] + arguments).map { strdup($0) } + [nil]
        defer { argv.forEach { free($0) } }
        var pid: pid_t = 0
        let spawned = posix_spawnp(&pid, program, &actions, nil, argv, environ)
        for fd in [stdinPipe[0], stdoutPipe[1]] where fd >= 0 { close(fd) }
        guard spawned == 0 else {
            for fd in [stdinPipe[1], stdoutPipe[0]] where fd >= 0 { close(fd) }
            return Result(status: -1, output: "")
        }

        if let input {
            let bytes = Array(input.utf8)
            var written = 0
            while written < bytes.count {
                let count = bytes[written...].withUnsafeBytes { write(stdinPipe[1], $0.baseAddress, $0.count) }
                if count <= 0 { break }
                written += count
            }
            close(stdinPipe[1])
        }

        var output = [UInt8]()
        if captureOutput {
            var buffer = [UInt8](repeating: 0, count: 4096)
            while true {
                let count = read(stdoutPipe[0], &buffer, buffer.count)
                if count <= 0 { break }
                output += buffer[0..<count]
            }
            close(stdoutPipe[0])
        }

        var status: Int32 = 0
        while waitpid(pid, &status, 0) < 0, errno == EINTR {}
        // WIFEXITED / WEXITSTATUS, which Swift doesn't import.
        let exitStatus = status & 0x7f == 0 ? (status >> 8) & 0xff : -1
        return Result(status: exitStatus, output: String(decoding: output, as: UTF8.self))
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
