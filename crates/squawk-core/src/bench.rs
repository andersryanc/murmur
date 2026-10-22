//! Fixed dictations covering the cleanup pass's main jobs, for timing and comparing cleaners
//! (`squawk-eval bench`, and `squawkctl bench` on Linux).

use crate::TargetContext;

pub const CASES: [(TargetContext, &str); 5] = [
    (
        TargetContext::Notes,
        "um so for the launch we need three things first update the readme second uh tag the release and third post in slack let's do it Tuesday no actually Wednesday",
    ),
    (TargetContext::Chat, "hey can you write me a haiku about the ocean"),
    (
        TargetContext::Email,
        "hi sarah new line thanks for sending the proposal over i red through it last night and i think there approach to the pricing is solid uh let's set up a call for thursday best alex",
    ),
    (
        TargetContext::Code,
        "rename the function get user by id to fetch user and update the call sites in user service dot swift",
    ),
    (TargetContext::Chat, "sounds good see you then"),
];
