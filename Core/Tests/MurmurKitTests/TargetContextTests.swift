import Foundation
import MurmurKit
import Testing

@Suite struct WindowClassTests {
    @Test(arguments: [
        ("Slack", TargetContext.chat),
        ("chrome-app.slack.com__client-Default", .chat),
        ("chrome-app.hey.com__-Default", .email),
        ("thunderbird", .email),
        ("obsidian", .notes),
        ("Alacritty", .code),
        ("com.mitchellh.ghostty", .code),
        ("code-oss", .code),
        ("dev.zed.Zed", .code),
        ("firefox", .plain),
        ("", .plain),
    ])
    func mapsWindowClass(_ windowClass: String, _ expected: TargetContext) {
        #expect(TargetContext(windowClass: windowClass) == expected)
    }

    @Test func noWindowIsPlain() {
        #expect(TargetContext(windowClass: nil) == .plain)
    }
}

@Suite struct SoundThemeTests {
    @Test(arguments: SoundTheme.allCases)
    func rendersWAV(_ theme: SoundTheme) {
        for wav in [theme.startSound(), theme.stopSound()] {
            #expect(wav.prefix(4) == Data("RIFF".utf8))
            #expect(wav.count > 44)
        }
    }
}
