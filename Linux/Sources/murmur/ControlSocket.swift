import Dispatch
import Foundation
import Glibc

/// The daemon listens on a Unix socket in `$XDG_RUNTIME_DIR/murmur/`. Each client connection
/// sends one command ("start", "stop", …) and reads back one line.
enum ControlSocket {
    /// Sends `command` to the running daemon and returns its reply.
    static func send(_ command: String) throws -> String {
        let fd = try connected()
        defer { close(fd) }
        let bytes = Array((command + "\n").utf8)
        guard write(fd, bytes, bytes.count) == bytes.count else { throw MurmurSocketError.io }
        shutdown(fd, Int32(SHUT_WR))
        var reply = [UInt8]()
        var buffer = [UInt8](repeating: 0, count: 512)
        while true {
            let count = read(fd, &buffer, buffer.count)
            if count <= 0 { break }
            reply += buffer[0..<count]
        }
        return String(decoding: reply, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Starts listening and calls `handler` on the main actor for each command. A client that
    /// sends "watch" stays connected and is handed to `watch` instead. Keep the returned source
    /// alive for as long as the daemon runs.
    static func listen(
        _ handler: @escaping @MainActor (String) -> String,
        watch: @escaping @MainActor (Int32) -> Void
    ) throws -> any DispatchSourceRead {
        let path = Paths.socket.path
        Paths.ensure(Paths.runtime)
        if (try? connected()).map({ close($0) }) != nil {
            throw MurmurSocketError.alreadyRunning
        }
        unlink(path)

        let fd = socket(AF_UNIX, Int32(SOCK_STREAM.rawValue), 0)
        guard fd >= 0 else { throw MurmurSocketError.io }
        var address = try address(path)
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard bound == 0, Glibc.listen(fd, 8) == 0 else { close(fd); throw MurmurSocketError.io }

        let source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: .main)
        source.setEventHandler {
            let client = accept(fd, nil, nil)
            guard client >= 0 else { return }
            var buffer = [UInt8](repeating: 0, count: 256)
            let count = read(client, &buffer, buffer.count)
            guard count > 0 else { close(client); return }
            let command = String(decoding: buffer[0..<count], as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            if command == "watch" {
                MainActor.assumeIsolated { watch(client) }
                return
            }
            defer { close(client) }
            let reply = MainActor.assumeIsolated { handler(command) } + "\n"
            _ = reply.utf8CString.withUnsafeBufferPointer { write(client, $0.baseAddress, $0.count - 1) }
        }
        source.setCancelHandler { close(fd); unlink(path) }
        source.resume()
        return source
    }

    private static func connected() throws -> Int32 {
        let fd = socket(AF_UNIX, Int32(SOCK_STREAM.rawValue), 0)
        guard fd >= 0 else { throw MurmurSocketError.io }
        var address = try address(Paths.socket.path)
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard result == 0 else { close(fd); throw MurmurSocketError.notRunning }
        return fd
    }

    private static func address(_ path: String) throws -> sockaddr_un {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        try withUnsafeMutableBytes(of: &address.sun_path) { buffer in
            guard bytes.count < buffer.count else { throw MurmurSocketError.io }
            buffer.copyBytes(from: bytes)
        }
        return address
    }
}

/// Clients that sent "watch", such as the Omarchy shell plugin that draws the on-screen pill.
/// Each gets one line per event: `state recording`, `level 0.42`, `notice …`.
@MainActor
final class Watchers {
    private var clients: [Int32] = []

    var isEmpty: Bool { clients.isEmpty }

    /// Starts sending events to `fd`, beginning with `greeting` (the current state).
    func add(_ fd: Int32, greeting: String) {
        clients.append(fd)
        send(greeting, to: [fd])
    }

    func send(_ line: String) {
        send(line, to: clients)
    }

    /// Drops any client that has gone away or stopped reading, rather than waiting on it.
    private func send(_ line: String, to targets: [Int32]) {
        let bytes = Array((line + "\n").utf8)
        for fd in targets where Glibc.send(fd, bytes, bytes.count, Int32(MSG_NOSIGNAL | MSG_DONTWAIT)) != bytes.count {
            close(fd)
            clients.removeAll { $0 == fd }
        }
    }
}

enum MurmurSocketError: Error, CustomStringConvertible {
    case notRunning, alreadyRunning, io

    var description: String {
        switch self {
        case .notRunning: "Murmur isn't running (or is still loading). Start it with `murmur daemon`."
        case .alreadyRunning: "Murmur is already running."
        case .io: "Couldn't talk to Murmur over \(Paths.socket.path)"
        }
    }
}
