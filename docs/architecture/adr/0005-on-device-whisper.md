# ADR-0005: On-device transcription with whisper.cpp

- Status: Accepted
- Date: 2026-11-08

## Context
Voice memos attached to links are personal. Sending audio to a cloud API would break Stow's "nothing leaves the machine unless you ask" promise, and it would need an account or key just to transcribe.

## Decision
- Record with `cpal`, resample to 16 kHz mono, and store the WAV with `hound`.
- Transcribe with `whisper-rs` (whisper.cpp), using Metal on Apple Silicon. `STOW_WHISPER_CPU=1` forces the CPU path (used in CI). The output is Markdown with timestamps.
- Models (`tiny.en` 78 MB, `base.en` 148 MB by default, `small.en` 488 MB) are **downloaded on first use** from the whisper.cpp model repository on Hugging Face into `models/`, with progress shown, or ahead of time with **Download now** in Settings → Transcription. They are not bundled. Downloads are pinned to one revision (`transcribe::MODEL_COMMIT`) and verified against the exact size and SHA-256 recorded in `transcribe::MODELS`; a mismatch deletes the file.
- Transcription is a background job (one at a time). Jobs cut off by a quit are marked `interrupted`; pending ones resume at startup, and a recording that has started three times is not retried again. The loaded model is cached in memory between runs.

## Consequences
- Audio never leaves the Mac. The only network request is the one-time model download.
- The DMG stays small. The first transcription waits for a download of 78–488 MB depending on the model.
- Building from source requires CMake, and the first compile is slow. The ggml Metal shaders are compiled at first use on the user's machine.
- Only English models are offered for now.
- A model file that fails verification is deleted and the download is retried; integrity does not rely on HTTPS alone.

## Alternatives considered
- **Apple Speech (`SFSpeechRecognizer`).** No download, but it needs a separate Speech Recognition permission and Objective-C bridging, quality and offline support vary by locale, and it isn't reusable from the CLI.
- **Cloud transcription (OpenAI, Deepgram).** Better accuracy on hard audio, but it sends audio off the machine. It could be added later as an explicit opt-in AI action.
- **Bundling a model.** Instant first use, but it adds 150 MB+ to every download, including for users who never record.
