# Squawk improvement plan

A task list of reliability, quality and project-hygiene improvements for Squawk, from a review of
the codebase. Nothing here is implemented yet.

- Reviewed at `d0a50a9` (after the rename to Squawk), on 2026-10-21. Line numbers are from that
  commit; if they've drifted, find the code by the symbol named.

## How to work through this

1. Start with **Decisions**. A few tasks need an answer from Ryan first.
2. Work **Tier 1** top to bottom, one commit per task, then Tier 2. Tier 3 is a menu, not a queue.
3. Each task gives the problem, where it is, what to change, and when it's done. When a task
   changes a crate's interface, update [CONTRACTS.md](../CONTRACTS.md) in the same commit.
4. Before each commit, run `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. Anything
   that touches audio, cleanup or the prompt also needs a `squawk-eval` run (see "Eval harness"
   in the README).
5. Tick a task's box when it lands. Move anything that turns into a longer-term idea into
   [ROADMAP.md](../ROADMAP.md).

## Keep these

These already work well, so don't regress them:

- **The queue.** You can dictate again while the last dictation is still processing, and they
  paste in order.
- **Atomic model download.** The download is staged, then renamed into place
  ([squawk-stt/src/lib.rs:51](../crates/squawk-stt/src/lib.rs:51)), so a half-finished download is
  never loaded.
- **One resampling pass** over the whole recording.
- **A listen-only key tap.** It never swallows or changes anyone else's keys.
- **The Dictionary matcher.** It treats spaces, hyphens and commas alike, and applies longer
  phrases first.

## Design principle for delivery

Paste once, after recording stops, into the app the dictation started in. These have each been
shown to cause more trouble than they solve:

- typing live while you speak;
- typing character by character;
- inserting through each app's Accessibility API;
- pasting line by line.

They break image pasting in terminals, leave partial text behind in some apps, and depend on the
keyboard layout.

## Decisions

These need an answer before the task named.

- [ ] **D1: when you've switched apps (T2.1).** When the app in front at paste time isn't the one
  you dictated into, recommended: copy only and say so ("Copied: you switched apps. Press ⌘V to paste").
- [ ] **D2: a stray key during a hold (T1.4).** Today an accidental key throws the whole recording
  away. Option: for a hold over ~3 s with speech in it, transcribe it into Recent without pasting.
- [ ] **D3: saved history (T2.6).** Storing transcripts on disk is a privacy choice. Recommended:
  on by default, with a retention limit and a menu switch to turn it off. Never log transcripts at
  info level.
- [ ] **D4: a licence (T1.8).** The repo has none, but Squawk.app ships the sherpa-onnx
  (Apache-2.0) and ONNX Runtime (MIT) libraries. Pick one, MIT or Apache-2.0.
- [ ] **D5: restoring the clipboard (T3.5).** This conflicts with the deliberate "the pasted text
  replaces your clipboard" ([ROADMAP.md](../ROADMAP.md), Known issues). Keep it as is, or offer a setting.

## Tier 1: small, high value

### T1.1 Stop Squawk's own ⌘V from cancelling the next dictation

- [ ] Done

**Problem (likely, not yet reproduced).** `send_command_v` posts ⌘V at the HID tap location
([macos.rs:397-404](../crates/squawk-platform/src/macos.rs:397)). Squawk's own listen-only tap sits
at that location too, and Apple documents that posted events pass through the taps there. `classify`
turns any `KeyDown` into `OtherKey` ([macos.rs:57-58](../crates/squawk-platform/src/macos.rs:57)),
and a held hotkey plus `OtherKey` emits `Cancelled` ([hold.rs:132](../crates/squawk-platform/src/hold.rs:132)).
So if dictation #1 pastes while you're holding the key for #2, #2 is thrown away. That is exactly
the flow the queue is for ([worker.rs:4-5](../crates/squawk-engine/src/worker.rs:4)).

**Reproduce first.** Run with debug logging. Dictate a long sentence with Claude cleanup, then
immediately hold fn again and keep talking for about 3 s. It's confirmed if the pill disappears the
moment #1 pastes and the log shows `hotkey: Cancelled`.

**Change.** Mark Squawk's synthetic events, and have the tap ignore them. Set the
`EventSourceUserData` field on both ⌘V events to a constant (e.g. `0x7371776B`, "sqwk"), and in
`tap_callback` ([macos.rs:344](../crates/squawk-platform/src/macos.rs:344)) skip any event that
carries it. If marking doesn't work, post ⌘V to the target app's process with
`CGEvent::post_to_pid`, which bypasses taps. Linux has no tap, so it's unaffected.

**Done when** the reproduction no longer cancels, and a unit test shows a marked event being ignored.

### T1.2 Keep recording briefly after the key comes up

- [ ] Done

**Problem.** `Engine::release` stops the recorder immediately
([squawk-engine/src/lib.rs:244-250](../crates/squawk-engine/src/lib.rs:244)), and `Recorder::stop`
shuts the stream down at once ([capture.rs:166-176](../crates/squawk-audio/src/capture.rs:166)).
Audio still in flight from the device is lost, which clips the last word.

**Change.** Keep the stream running for a short tail between the key coming up and the stream
stopping. Start with a fixed 80 ms. Optionally make it adaptive: the longest recent callback
interval or buffer duration plus 8 ms, clamped to 20–80 ms. Prefer not to block the main thread:
for example, let the worker stop the recorder once the tail has passed. Add the constant to
CONTRACTS.md beside `MINIMUM_HOLD`.

**Done when** `squawk-eval run` shows no WER regression, and clips that end on a short word keep it.

### T1.3 Put a space between consecutive dictations

- [ ] Done

**Problem.** The worker pastes the text exactly as cleanup returned it
([worker.rs:111](../crates/squawk-engine/src/worker.rs:111)), so two dictations in a row read
"One.Two."

**Change.** Append a trailing space to the pasted text unless it ends in a newline. Keep the space
out of Recent. Do it in the worker so it works on every platform, not in `squawk-platform`.

**Done when** two quick dictations read "One. Two." in a text field and in a terminal.

### T1.4 Never drop a dictation

- [ ] Done (the third case needs D2)

**Problems.** In each of these cases your words disappear without a trace:

- If the paste fails, the worker returns before `Event::Dictated`
  ([worker.rs:111-120](../crates/squawk-engine/src/worker.rs:111)), so the text isn't in Recent either.
- If transcription fails, the audio is thrown away ([worker.rs:53-58](../crates/squawk-engine/src/worker.rs:53)).
- If another key is pressed during a hold, the recording is thrown away
  ([lib.rs:288-293](../crates/squawk-engine/src/lib.rs:288)), however long it was.

**Change.**

- On a paste error, still record the text (as `Dictated`, or with an outcome; see T2.6), and say
  where it went: "Couldn't paste; it's in Recent".
- On a transcription error, save the samples as a WAV under `data_dir()/failed/` and say so.
  `save_wav` already exists in [squawk-eval/src/clip.rs:74](../crates/squawk-eval/src/clip.rs:74);
  move it into `squawk-audio` beside `load_wav`.
- Per D2, transcribe long cancelled holds into Recent without pasting. This needs a "don't paste"
  flag on `Job`.
- Update what `Event::Dictated` means in CONTRACTS.md.

**Done when** each forced failure (a paste error, a corrupt model, a chord after 5 s) leaves the
text or the audio somewhere you're told about.

### T1.5 Explain an unresponsive hotkey while the model loads, and allow a retry

- [ ] Done

**Problem.** On macOS, pressing the hotkey before the model is ready does nothing and shows nothing
([tray/app.rs:318-319](../crates/squawk/src/tray/app.rs:318)). The Linux daemon already replies
"loading". A failed load (`ModelState::Failed`, [model.rs:81](../crates/squawk-engine/src/model.rs:81))
lasts until you relaunch, and the menu has no Retry.

**Change.**

- On a press before the model is ready, flash "Speech model is loading…" or the failure text in
  the pill. `flash` is at [app.rs:335](../crates/squawk/src/tray/app.rs:335).
- Add an engine call that reloads the model after a failure, and a "Retry Loading the Model" menu
  item while it's `Failed`.
- Give the Linux daemon the same call (for example on `reload`).
- Update CONTRACTS.md.

**Done when** a press during loading shows a message, and after you delete a model file, Retry
recovers it without a relaunch.

### T1.6 Cap the Ollama reply

- [ ] Done

**Problem.** The Ollama request sets only the temperature
([cleaner.rs:262-271](../crates/squawk-core/src/cleaner.rs:262)), so a model that rambles is
pasted in full, after up to 60 s. Claude's reply is already capped at 4096 tokens, and a truncated
reply is treated as a failure ([cleaner.rs:127](../crates/squawk-core/src/cleaner.rs:127),
[:158](../crates/squawk-core/src/cleaner.rs:158)).

**Change.** Send `options.num_predict = min(8192, max(1024, 256 + 8 × words))`, and fail when the
response has `done_reason: "length"`, as the Claude path does. Optionally size Claude's
`max_tokens` the same way; its test is at [cleaner.rs:302](../crates/squawk-core/src/cleaner.rs:302).

**Done when** a unit test checks the request body and the length failure.

### T1.7 Add guardrails to CLAUDE.md

- [ ] Done

[CLAUDE.md](../CLAUDE.md) says how to build and test Squawk, but not what must stay true or which
build traps are easy to fall back into. Here is a draft to adapt:

```markdown
- **A spoken word never disappears.** If cleanup, the API key or the paste fails, paste the raw
  (Dictionary-corrected) transcript or leave it on the clipboard and in Recent, and say so
  (`squawk-engine/src/worker.rs`, `prompt::finalize`).
- **Privacy.** Audio never leaves the machine; only the transcript (and Dictionary words) go to
  the chosen cleaner. Never log transcripts at info level or above, and never log the API key or
  request headers.
- **Models are downloaded, never bundled or committed.** Eval clips and `eval/results/` stay local.
- **Build the Mac app with `scripts/bundle-macos.sh` only.** The binary needs
  `libsherpa-onnx-c-api.dylib` and `libonnxruntime.dylib` in `Contents/Frameworks` (rpath from
  `crates/squawk/build.rs`), signed before the app, with `disable-library-validation` for
  self-signed builds. A bare `cargo build` binary is not an app.
- **Permissions follow the bundle ID and the signing certificate.** Changing either makes every
  user grant Microphone and Accessibility again; don't do it without a note in the README.
- **Prompt and cleaner changes need evidence.** Run `squawk-eval` before and after; every cleanup
  bug gets a case in the cleanup corpus.
- **Tests that need the model, an API key or a desktop are `#[ignore]`d.** Run
  `cargo test -- --ignored` on a real machine before changing those paths.
- **Paste once, after recording stops, into the app the dictation started in.** Don't add live
  typing or character-by-character insertion: they break image pasting and leave partial text behind.
```

**Done when** CLAUDE.md has these bullets, trimmed to match its style.

### T1.8 Add a licence and third-party notices

- [ ] Done (needs D4)

**Change.**

- Add `LICENSE`.
- Add `THIRD_PARTY_NOTICES.md` covering sherpa-onnx (Apache-2.0), ONNX Runtime (MIT), and the
  Parakeet TDT 0.6B v3 model. Check the model card for its licence and the attribution it asks for.
- Have `bundle-macos.sh` copy both files into `Contents/Resources`.
- Add a licence line to the README.

## Tier 2: medium

### T2.1 Paste only into the app the dictation started in

- [ ] Done (needs D1)

**Problem.** A `Job` keeps only the `TargetContext` ([worker.rs:27](../crates/squawk-engine/src/worker.rs:27)).
The paste goes to whatever is in front once cleanup finishes
([macos.rs:378](../crates/squawk-platform/src/macos.rs:378),
[linux.rs:113-115](../crates/squawk-platform/src/linux.rs:113)). That can be up to 20 s later with
Claude or 60 s with Ollama ([cleaner.rs:110](../crates/squawk-core/src/cleaner.rs:110),
[:200](../crates/squawk-core/src/cleaner.rs:200)), plus any time spent waiting in the queue.

**Change.**

- In `squawk-platform`, add `frontmost_target() -> Option<Target>`: the pid and bundle ID on macOS,
  and the window `address` from `hyprctl activewindow -j` on Linux.
- Capture the target at press time, next to `target_context()`
  ([app.rs:328](../crates/squawk/src/tray/app.rs:328), and in the Linux daemon's start handler),
  and store it in the `Job`.
- Before pasting, compare it with the current target. If it changed, put the text on the clipboard
  only, and return a new `PasteOutcome::FocusChanged` with the D1 notice.
- Keep the comparison a pure function so it can be unit-tested.
- Make sure the recording pill never becomes the frontmost app.
- Update CONTRACTS.md: the `squawk-platform` section, and the `Job`/`press` flow.

**Done when** the text ends up on the clipboard with a notice, on both macOS and Hyprland, after
you dictate into one app, switch to another before the paste lands, and come back.

### T2.2 Check the cleaner's reply before pasting it

- [ ] Done

**Problem.** Squawk pastes whatever the cleaner returns, as long as it isn't empty
([prompt.rs:17](../crates/squawk-core/src/prompt.rs:17)). Checks that require the output to keep
every input word don't fit Squawk's cleanup, which rewrites freely and formats lists. These do:

| Check | What it catches |
|---|---|
| Dictionary terms survive: every vocabulary word or replacement target found in the input appears at least as often in the output | the model "correcting" `kubectl` back into words |
| Facts survive: numbers, URLs, emails and paths. Treat `1,000` and `1000` as equal, and skip the check when a self-correction cue is present ("no", "actually", "sorry", "scratch that", "I mean") | dropped or altered facts |
| Strip an invented leading `#` heading, unless the transcript says "heading" or "title" | the Ollama issue in ROADMAP's Known issues |
| Strip a code fence that wraps the whole reply | the model fencing its own answer |
| Answered instead of cleaned: too many output words appear in neither the input nor the vocabulary, or the output is much longer than the input | "what's the capital of France" → "Paris" |

**Change.** Add a `guard` module to `squawk-core` and run it after `finalize`.

- The two strip rules rewrite the reply.
- The other checks pass or fail. On a failure, log which check failed, then paste the
  Dictionary-corrected raw transcript with the notice "Cleanup changed too much, pasted raw text".
- Ship the "answered instead of cleaned" check as log-only, until the corpus shows its thresholds
  are safe.
- Add the module to CONTRACTS.md.

**Done when** each check has unit tests, and every `expected` output in the T2.3 corpus passes all
the checks. That proves there are no false positives on known-good cleanups.

### T2.3 A text-only cleanup corpus that runs in `cargo test`

- [ ] Done

**Problem.** `squawk_core::bench::CASES` ([bench.rs:6](../crates/squawk-core/src/bench.rs:6)) is
five transcripts with no expected output, so nothing fails visibly when cleanup regresses.

**Change.** Add `eval/cleanup.json`, with entries of the form
`{id, context, transcript, vocabulary?, expected, required[], forbidden[]}`. Ids starting
`negative-` should come back essentially unchanged; ids starting `empty-` should come back as
`<empty/>`. You can turn a fixture into a stress case by repeating it with a `{{index}}`
placeholder.

- Seed it with the known cases:
  - "yeah okay";
  - a notes dictation that tempts a heading;
  - "what's the capital of France";
  - a spoken list;
  - "Tuesday, no actually Wednesday";
  - a code identifier;
  - filler-only input.
- In `cargo test`, with no API key or network, check that:
  - the ids are unique;
  - every `TargetContext` is covered;
  - each `required` string appears in `expected`, and no `forbidden` string does;
  - `finalize`, `is_only_filler`, `apply_replacements` and the T2.2 checks agree with every case.
- In `squawk-eval`, add a command that runs a cleaner over the corpus and reports:
  - matches against `expected`;
  - required and forbidden pass rates;
  - words changed;
  - T2.2 failures;
  - "AI smell" phrases ("Here's the cleaned", "I've", "Certainly");
  - p50 and p95 latency by transcript length.
- Give the command pass/fail thresholds: 100% on `negative-` and `empty-` cases, plus a p95
  latency budget.
- Point `bench` at the corpus, and retire `CASES`.

**Done when** `cargo test` validates the corpus, and the new command prints a scored table for
Claude and for `ollama:qwen3.5:9b`.

### T2.4 Score cleanup against `intended` in `squawk-eval run`

- [ ] Done

**Problem.**

- The report scores only the raw transcript ([report.rs:126](../crates/squawk-eval/src/report.rs:126)).
  It prints each clip's `intended` text but never scores against it
  ([report.rs:137](../crates/squawk-eval/src/report.rs:137)).
- Results are gitignored, so claims like ROADMAP's "~0.45 s" have no evidence in the repo.

**Change.**

- Add a WER of the cleaned output against `intended`, plus the T2.3 metrics.
- Add a committed `eval/reports/YYYY-MM-DD-topic.md` convention. Each report covers the machine,
  the model and prompt versions, the dataset, the summary table, conclusions, and a "Reproduce"
  section.
- Keep the raw JSON local.
- Optionally, add a mode that rescores saved outputs without calling a model. That catches drift
  in the scorer or the fixtures cheaply.

**Done when** `run` prints both WERs, and the first committed report exists.

### T2.5 Build and launch the app in CI

- [ ] Done

**Problem.** CI runs fmt, clippy and tests ([.github/workflows/rust.yml](../.github/workflows/rust.yml)),
but never builds Squawk.app. Bundling has broken in ways that tests can't catch:
`disable-library-validation` had to be added before self-signed builds would launch.

**Change.**

- Let `bundle-macos.sh` accept `SQUAWK_SIGN_IDENTITY=-` (ad-hoc), and skip the Keychain lookup for
  it ([bundle-macos.sh:24-30](../scripts/bundle-macos.sh:24)).
- Add a macOS job that reuses the sherpa-onnx download step, runs the script, and then checks that:
  - both dylibs are in `Contents/Frameworks`;
  - `otool -L` resolves them through `@rpath`;
  - `codesign --verify --deep --strict` passes;
  - `Contents/MacOS/Squawk` is still running after 6 s. Without Accessibility it just waits, which
    is fine.
- Cancel superseded runs on the same ref (`concurrency` with `cancel-in-progress`).

**Done when** the job passes, and fails on a branch that deletes a dylib.

### T2.6 Keep Recent on disk, with outcome and raw text

- [ ] Done (needs D3)

**Problem.** Recent lives in memory only: 10 entries on macOS ([app.rs:31](../crates/squawk/src/tray/app.rs:31))
and 5 in the Linux daemon ([daemon.rs:32](../crates/squawk/src/linux/daemon.rs:32)). It holds only
successful pastes, and it's lost on quit. ROADMAP's "Fix…" item needs the raw transcript kept next
to the cleaned one.

**Change.**

- Write `data_dir()/history.jsonl`, capped by count and by age (for example 90 days).
- Write one entry per dictation: `{time, app, context, raw, cleaned, outcome, cleaner, timings}`.
- Use these outcomes: `pasted`, `copied`, `focus_changed`, `paste_failed`, `cancelled`,
  `cleanup_failed`.
- Have both front ends read from the file.
- Decide which crate owns it, and add it to CONTRACTS.md.

**Done when** Recent survives a restart and shows failed dictations with their outcome.

### T2.7 Keep the hotkey tap alive

- [ ] Done

**Problem.** The tap is re-enabled only when macOS sends a `TapDisabled*` event
([macos.rs:198](../crates/squawk-platform/src/macos.rs:198)). Event taps can also go quiet after
sleep, wake or a user switch without any such event.

**Change.** On each `IDLE_WAIT` pass of the tap thread, check `CGEvent::tap_is_enabled`, and
re-enable or rebuild the tap if it's off. Also rebuild it after wake.

**Done when** the hotkey still works after a sleep and wake, and after switching to another user
and back.

## Tier 3: consider

Each of these is worth doing only if it earns its place. Most need an eval run before and after.

- [ ] **T3.1 Deterministic cleanup for Cleanup Off and as the fallback.**
  - Remove filler sounds and repeated fragments of up to about 8 words.
  - Keep intentional repeats like "yeah yeah" and "very very".
  - Add a missing final full stop to prose, but leave technical endings alone (paths, URLs, code).
  - Don't run it before Claude: the prompt does this better.
- [ ] **T3.2 Skip the LLM for very short transcripts.** For three words or fewer after removing
  fillers. This saves about a second, and sidesteps Ollama dropping "yeah". Measure with T2.3.
- [ ] **T3.3 Voice submit.** A trailing standalone "enter" or "send" is stripped before cleanup,
  and Return is pressed once the paste lands (wait ~40 ms and re-check focus).
  - A bare "send" stays as text, and so does "sender".
  - Keep it off by default, because "…I'll send" would submit.
- [ ] **T3.4 Spoken "lowercase" to continue a sentence.** A leading "lowercase" is consumed, and the
  next word starts in lower case. Lower the first letter again after the LLM, because models
  capitalize it again.
- [ ] **T3.5 Restore the clipboard (D5).** As a setting: snapshot every type on the clipboard, paste,
  and restore the snapshot after 300 ms, but only if the clipboard's `changeCount` hasn't moved since.
- [ ] **T3.6 Layout-aware ⌘V (Known issue).** Find the key code that types "v" in the current layout
  with `UCKeyTranslate`, falling back to 9. Linux is fine: `wtype -k v` sends a keysym.
- [ ] **T3.7 A mic unplugged mid-recording.** The stream error is only logged
  ([capture.rs:246](../crates/squawk-audio/src/capture.rs:246)). End the recording, queue what was
  captured, and show a notice.
- [ ] **T3.8 Pre-roll for the first word.** Open the mic on key-down instead of after the 200 ms hold
  ([hold.rs:12](../crates/squawk-platform/src/hold.rs:12)), and keep a short buffer. The cost is the
  mic indicator flickering on every fn tap.
- [ ] **T3.9 Transcribe while still talking (ROADMAP).**
  - Split the recording at pauses, and decode finished segments while recording continues, so only
    the tail is left at release.
  - Keep a final offline decode.
  - A live preview fed by a streaming model doesn't shorten the wait after release, so it doesn't
    serve this goal.
- [ ] **T3.10 Dictionary extras:**
  - plain-text import, one `heard -> replacement` per line;
  - replacements limited to a context (e.g. Code only);
  - re-applying a replacement's casing after the LLM.

  For ROADMAP's "Fix…", validate a correction before saving it:
  - "heard" must actually appear in the dictation being fixed;
  - collapse spelled-out letters ("p-o-r-t-m-a-n");
  - let an explicit casing word ("title case", "lowercase") override.
- [ ] **T3.11 Onboarding (ROADMAP).** Each step is derived from the real state of the Microphone and
  Accessibility permissions, the model and the API key. A test dictation can be skipped.
- [ ] **T3.12 Paced typing into Codex and Claude Code.** Only if their "[Pasted text]" placeholder
  for long pastes bothers you. Type in 16-character chunks 12 ms apart into terminals: Codex treats
  keystrokes 8 ms or less apart as a paste. Terminals already stop pasted newlines from running
  commands (bracketed paste), so the gain is only cosmetic.

## Considered and rejected

- **Local-only models, dropping Claude.** Squawk's default is Claude cleanup by design.
- **Checks that require the output to keep every input word**, including edit lists and
  "re-project the input's words onto the output". They can't express lists or self-corrections.
- **Regex corrections fitted to specific test phrases.** They overfit and rot.
- **Filtering the vocabulary to words found in the transcript** before prompting. It defeats
  near-miss corrections, which are what the vocabulary is for.
- **Live or character-by-character typing.** See the design principle above.
- **Double-tap to start, cycling through modes with a shortcut, and a stats dashboard.**
- **Release machinery: Developer ID, notarization, Sparkle.** Squawk is build-it-yourself
  (ROADMAP). If that changes:
  - pin the update feed URL and signing key from the very first release;
  - publish a draft release with a `.sha256`;
  - only make it public after CI has checked the artifact against the feed.
- **A source-size limit script.** The largest Rust file is about 570 lines; revisit if files grow.

## README and a site

There's no website, and the README is a build guide. To make it work as a landing page, add:

- [ ] A short privacy section: audio never leaves the machine; with Claude cleanup, the transcript
  and your Dictionary words go to Anthropic's API; Local Model or Off keeps everything on the machine.
- [ ] A licence line (after T1.8), and a table of what gets downloaded: the Parakeet model
  (~480 MB) and, optionally, Ollama models.
- [ ] Later, if wanted, a one-page site built from the README's pitch and `docs/screenshots/demo.gif`.
