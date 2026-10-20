import Foundation
import MurmurKit
import Testing

@Suite struct ReplacementTests {
    private func apply(_ rules: [(String, String)], to text: String) -> String {
        PersonalDictionary(replacements: rules.map { .init(from: $0.0, to: $0.1) }).applyReplacements(to: text)
    }

    @Test func ignoresCase() {
        #expect(apply([("cube control", "kubectl")], to: "Run Cube Control get pods") == "Run kubectl get pods")
    }

    @Test func treatsSpacesHyphensAndCommasAlike() {
        #expect(apply([("cube control", "kubectl")], to: "cube-control, cube,  control") == "kubectl, kubectl")
    }

    @Test func onlyMatchesWholeWords() {
        #expect(apply([("art", "ART")], to: "start the art party") == "start the ART party")
    }

    @Test func appliesLongerPhrasesFirst() {
        let rules = [("ply", "Ply"), ("ply hydrant", "Plyhdrn")]
        #expect(apply(rules, to: "ply hydrant and ply") == "Plyhdrn and Ply")
    }

    @Test func writesReplacementLiterally() {
        #expect(apply([("dollar sign one", "$1 \\0")], to: "costs dollar sign one") == "costs $1 \\0")
    }

    @Test func treatsPatternCharactersInFromLiterally() {
        #expect(apply([("c++", "C++")], to: "I like c++ a lot") == "I like C++ a lot")
    }

    @Test func skipsBlankRules() {
        #expect(apply([("", "x"), ("  ", "y")], to: "unchanged") == "unchanged")
    }

    @Test func keepsPunctuationAroundMatch() {
        #expect(apply([("my email", "me@example.com")], to: "Send it to my email.") == "Send it to me@example.com.")
    }
}

@Suite struct DictionaryFileTests {
    @Test func roundTripsAsPlainJSON() throws {
        let url = FileManager.default.temporaryDirectory.appending(path: "murmur-test-\(UUID()).json")
        defer { try? FileManager.default.removeItem(at: url) }
        let dictionary = PersonalDictionary(words: [.init("Plyhdrn")], replacements: [.init(from: "cube control", to: "kubectl")])
        try dictionary.save(to: url)

        let json = try String(contentsOf: url, encoding: .utf8)
        #expect(json.contains(#""words" : ["#) && json.contains(#""Plyhdrn""#))
        #expect(!json.contains("id"))
        #expect(try PersonalDictionary.load(from: url) == dictionary)
    }

    @Test func missingFileLoadsEmpty() throws {
        let url = FileManager.default.temporaryDirectory.appending(path: "does-not-exist-\(UUID()).json")
        #expect(try PersonalDictionary.load(from: url) == PersonalDictionary())
    }

    @Test func vocabularyIncludesReplacementTargetsWithoutDuplicates() {
        let dictionary = PersonalDictionary(
            words: [.init("kubectl"), .init("Plyhdrn"), .init(" ")],
            replacements: [.init(from: "cube control", to: "kubectl"), .init(from: "my email", to: "me@example.com")]
        )
        #expect(dictionary.vocabulary == ["kubectl", "Plyhdrn", "me@example.com"])
    }
}

@Suite struct FillerTests {
    @Test(arguments: ["um", "Uh, um.", "ummm", "hmm", "", "  "])
    func fillerOnly(_ text: String) { #expect(CleanupPrompt.isOnlyFiller(text)) }

    @Test(arguments: ["yes", "Yeah, okay.", "um yes", "ok"])
    func realWords(_ text: String) { #expect(!CleanupPrompt.isOnlyFiller(text)) }

    @Test func emptyMarkerKeepsRealWords() {
        #expect(CleanupPrompt.finalize("<empty/>", transcript: "Yeah.") == "Yeah.")
        #expect(CleanupPrompt.finalize("`<empty/>`", transcript: "Um.") == "")
        #expect(CleanupPrompt.finalize(" Hello. ", transcript: "hello") == "Hello.")
    }
}
