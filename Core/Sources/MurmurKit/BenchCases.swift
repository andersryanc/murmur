/// Fixed dictations covering the cleanup pass's main jobs, for timing and comparing cleaners
/// (`MurmurEval bench` on the Mac, `murmur bench` on Linux).
public let benchCases: [(TargetContext, String)] = [
    (.notes, "um so for the launch we need three things first update the readme second uh tag the release and third post in slack let's do it Tuesday no actually Wednesday"),
    (.chat, "hey can you write me a haiku about the ocean"),
    (.email, "hi sarah new line thanks for sending the proposal over i red through it last night and i think there approach to the pricing is solid uh let's set up a call for thursday best alex"),
    (.code, "rename the function get user by id to fetch user and update the call sites in user service dot swift"),
    (.chat, "sounds good see you then"),
]
