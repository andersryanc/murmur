import Foundation

/// Sample rate every engine takes. Parakeet and Apple's model both run at 16 kHz mono.
public let sttSampleRate: Double = 16_000

/// A cheap check for whether a recording contains any speech at all.
///
/// Speech models guess words from silence or room noise (Parakeet tends to hear "Yeah."), so a
/// quick tap of the hotkey would otherwise paste a phantom word. Murmur skips transcription when
/// there's too little voiced audio.
public enum VoiceActivity {
    /// 20 ms frames louder than this count as voiced. Speech into a laptop or headset mic is
    /// usually -40 to -15 dBFS; a quiet room is around -60 dBFS.
    public static let thresholdDB: Float = -50
    /// Less voiced audio than this is treated as no speech. A quick, soft "yes" has ~120 ms of
    /// voice; a keyboard click ~20 ms.
    public static let minimumSpeech: TimeInterval = 0.08

    /// Seconds of audio above `thresholdDB`, for 16 kHz mono samples.
    public static func voicedDuration(_ samples: [Float], sampleRate: Double = sttSampleRate) -> TimeInterval {
        let frame = Int(sampleRate * 0.02)
        guard frame > 0, samples.count >= frame else { return 0 }
        let threshold = pow(10, thresholdDB / 20)
        var voicedFrames = 0
        var start = 0
        while start + frame <= samples.count {
            var sum: Float = 0
            for i in start..<(start + frame) { sum += samples[i] * samples[i] }
            if (sum / Float(frame)).squareRoot() > threshold { voicedFrames += 1 }
            start += frame
        }
        return Double(voicedFrames) * 0.02
    }

    public static func containsSpeech(_ samples: [Float], sampleRate: Double = sttSampleRate) -> Bool {
        voicedDuration(samples, sampleRate: sampleRate) >= minimumSpeech
    }
}
