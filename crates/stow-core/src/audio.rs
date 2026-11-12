//! Audio capture (cpal) and 16 kHz mono WAV helpers (hound).
//!
//! The recorder runs on a dedicated std thread because `cpal::Stream` is not `Send`
//! on macOS; `RecorderHandle` talks to it over `std::sync::mpsc` and is safe to hold
//! in Tauri managed state. The realtime capture callback never takes a lock: it hands
//! sample chunks to a writer thread over a bounded channel with `try_send`. The writer
//! resamples each chunk to 16 kHz mono as it arrives and appends it to the WAV, so a
//! recording's memory use does not grow with its length; [`MAX_RECORDING`] caps the
//! length anyway and the recorder stops itself there.

use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

use crate::error::{IoContext, Result, StowError};

/// Sample rate Whisper expects.
pub const TARGET_HZ: u32 = 16_000;

/// Longest recording. The recorder stops itself here and reports the finished WAV
/// through [`RecorderHandle::take_auto_stopped`] (and the hook given to
/// [`RecorderHandle::spawn_with_limit`]).
pub const MAX_RECORDING: Duration = Duration::from_secs(3 * 60 * 60);

fn audio_err(e: impl std::fmt::Display) -> StowError {
    StowError::Audio(e.to_string())
}

// DSP over audio buffers: lengths are far below 2^23 samples per second of audio and
// sample values are in [-1, 1], so these float/int conversions cannot lose meaningful
// precision or wrap.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
/// Resample by linear interpolation. `from_hz == to_hz` returns a copy. When
/// downsampling, a centred moving average of `ceil(from/to)` samples runs first as a
/// cheap anti-alias low-pass (its first null sits at `to_hz`); upsampling is plain linear.
/// Output length is always `len * to / from`. [`Resampler`] is the same computation
/// fed one chunk at a time.
pub fn resample_linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || input.is_empty() || from_hz == 0 || to_hz == 0 {
        return input.to_vec();
    }
    let filtered;
    let src: &[f32] = if from_hz > to_hz {
        filtered = box_lowpass(input, from_hz.div_ceil(to_hz) as usize);
        &filtered
    } else {
        input
    };
    let out_len = (input.len() as u64 * u64::from(to_hz) / u64::from(from_hz)) as usize;
    let step = f64::from(from_hz) / f64::from(to_hz);
    let last = src.len() - 1;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let idx = (pos.floor() as usize).min(last);
            let frac = (pos - idx as f64) as f32;
            let a = src[idx];
            let b = src[(idx + 1).min(last)];
            a + (b - a) * frac
        })
        .collect()
}

// DSP over audio buffers: lengths are far below 2^23 samples per second of audio and
// sample values are in [-1, 1], so these float/int conversions cannot lose meaningful
// precision or wrap.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
/// Moving average over `window` samples centred on each input sample, edges clamped.
fn box_lowpass(input: &[f32], window: usize) -> Vec<f32> {
    if window <= 1 {
        return input.to_vec();
    }
    let last = input.len() as isize - 1;
    let half = (window / 2) as isize;
    // Even windows take one more sample ahead than behind; the half-sample shift is far
    // below anything audible at speech sample rates.
    let lo_off = -half;
    let hi_off = window as isize - 1 - half;
    let at = |i: isize| input[i.clamp(0, last) as usize];
    // Prime the running sum for sample 0, then slide: O(n) regardless of window size.
    let mut sum: f32 = (lo_off..=hi_off).map(at).sum();
    let scale = 1.0 / window as f32;
    (0..input.len() as isize)
        .map(|i| {
            let v = sum * scale;
            sum += at(i + 1 + hi_off) - at(i + lo_off);
            v
        })
        .collect()
}

/// [`box_lowpass`] fed one sample at a time. Output `i` needs input `i + hi_off`, so
/// the stream lags that far behind; `finish` flushes the tail with the last sample
/// repeated (the batch version's edge clamp).
struct StreamLowpass {
    window: usize,
    scale: f32,
    /// The `window` virtual samples around the next output (clamped copies at the edges).
    win: VecDeque<f32>,
    sum: f32,
    received: usize,
    emitted: usize,
    last: f32,
}

/// Recompute the running sum this often so f32 drift cannot build up over hours.
const LOWPASS_RESUM_EVERY: usize = 4096;

#[allow(clippy::cast_precision_loss)] // window is tiny
impl StreamLowpass {
    fn new(window: usize) -> StreamLowpass {
        StreamLowpass {
            window,
            scale: 1.0 / window as f32,
            win: VecDeque::with_capacity(window + 1),
            sum: 0.0,
            received: 0,
            emitted: 0,
            last: 0.0,
        }
    }

    fn push(&mut self, x: f32, out: &mut Vec<f32>) {
        if self.received == 0 {
            // Indices -half..-1 read as sample 0 in the batch version.
            for _ in 0..self.window / 2 {
                self.add(x, out);
            }
        }
        self.received += 1;
        self.last = x;
        self.add(x, out);
    }

    fn add(&mut self, x: f32, out: &mut Vec<f32>) {
        self.win.push_back(x);
        self.sum += x;
        if self.win.len() == self.window {
            if self.emitted.is_multiple_of(LOWPASS_RESUM_EVERY) {
                self.sum = self.win.iter().sum();
            }
            out.push(self.sum * self.scale);
            self.emitted += 1;
            if let Some(old) = self.win.pop_front() {
                self.sum -= old;
            }
        }
    }

    fn finish(&mut self, out: &mut Vec<f32>) {
        while self.emitted < self.received {
            self.add(self.last, out);
        }
    }
}

/// The interpolation half of [`resample_linear`] fed one chunk at a time: keeps the
/// source samples the next output still needs, and the absolute output index so
/// positions are the batch version's exactly.
struct StreamLinear {
    step: f64,
    from_hz: u32,
    to_hz: u32,
    /// Source samples `base..received`.
    src: Vec<f32>,
    base: usize,
    received: usize,
    next_out: usize,
}

// Same reasoning as `resample_linear`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
impl StreamLinear {
    fn new(from_hz: u32, to_hz: u32) -> StreamLinear {
        StreamLinear {
            step: f64::from(from_hz) / f64::from(to_hz),
            from_hz,
            to_hz,
            src: Vec::new(),
            base: 0,
            received: 0,
            next_out: 0,
        }
    }

    fn idx_of(&self, out_index: usize) -> (usize, f32) {
        let pos = out_index as f64 * self.step;
        let idx = pos.floor() as usize;
        (idx, (pos - idx as f64) as f32)
    }

    fn push(&mut self, xs: &[f32], out: &mut Vec<f32>) {
        self.src.extend_from_slice(xs);
        self.received += xs.len();
        // Emit every output whose right-hand neighbour has arrived and that the final
        // length (`total * to / from`, which only grows) is sure to include.
        let cap = self.out_len();
        loop {
            let (idx, frac) = self.idx_of(self.next_out);
            if self.next_out >= cap || idx + 1 >= self.received {
                break;
            }
            let a = self.src[idx - self.base];
            let b = self.src[idx + 1 - self.base];
            out.push(a + (b - a) * frac);
            self.next_out += 1;
        }
        // Forget samples no later output can read.
        let (idx, _) = self.idx_of(self.next_out);
        let drop = idx.saturating_sub(self.base).min(self.src.len());
        if drop > 0 {
            self.src.drain(..drop);
            self.base += drop;
        }
    }

    /// `len * to / from` for the input so far.
    fn out_len(&self) -> usize {
        (self.received as u64 * u64::from(self.to_hz) / u64::from(self.from_hz)) as usize
    }

    /// The outputs past the last source sample all clamp to it (`(idx + 1).min(last)`).
    fn finish(&mut self, out: &mut Vec<f32>) {
        let out_len = self.out_len();
        if let Some(&last) = self.src.last() {
            while self.next_out < out_len {
                out.push(last);
                self.next_out += 1;
            }
        }
    }
}

/// [`resample_linear`] over a stream: feed mono chunks with `push`, then `finish`. The
/// concatenated output is the batch result up to f32 rounding (see the tests).
pub struct Resampler {
    stages: Option<(Option<StreamLowpass>, StreamLinear)>,
    scratch: Vec<f32>,
}

impl Resampler {
    /// `from_hz == to_hz` (or a zero rate) passes samples through unchanged, like the
    /// batch version.
    pub fn new(from_hz: u32, to_hz: u32) -> Resampler {
        let stages = (from_hz != to_hz && from_hz != 0 && to_hz != 0).then(|| {
            let lowpass =
                (from_hz > to_hz).then(|| StreamLowpass::new(from_hz.div_ceil(to_hz) as usize));
            (lowpass, StreamLinear::new(from_hz, to_hz))
        });
        Resampler {
            stages,
            scratch: Vec::new(),
        }
    }

    /// Resample `mono` and append what can be produced so far to `out`.
    pub fn push(&mut self, mono: &[f32], out: &mut Vec<f32>) {
        let Some((lowpass, linear)) = &mut self.stages else {
            out.extend_from_slice(mono);
            return;
        };
        match lowpass {
            Some(lp) => {
                self.scratch.clear();
                for &x in mono {
                    lp.push(x, &mut self.scratch);
                }
                linear.push(&self.scratch, out);
            }
            None => linear.push(mono, out),
        }
    }

    /// Append the remaining output; the total length is then `pushed * to / from`.
    pub fn finish(&mut self, out: &mut Vec<f32>) {
        let Some((lowpass, linear)) = &mut self.stages else {
            return;
        };
        if let Some(lp) = lowpass {
            self.scratch.clear();
            lp.finish(&mut self.scratch);
            linear.push(&self.scratch, out);
        }
        linear.finish(out);
    }
}

// DSP over audio buffers: lengths are far below 2^23 samples per second of audio and
// sample values are in [-1, 1], so these float/int conversions cannot lose meaningful
// precision or wrap.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
/// Average interleaved channels into one. A trailing partial frame is dropped.
pub fn to_mono(frames: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return frames.to_vec();
    }
    let n = channels as usize;
    frames
        .chunks_exact(n)
        .map(|frame| frame.iter().sum::<f32>() / n as f32)
        .collect()
}

fn wav_spec_16k() -> hound::WavSpec {
    hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_HZ,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

/// An open 16 kHz mono i16 WAV being appended to.
struct WavSink {
    writer: hound::WavWriter<BufWriter<File>>,
    samples: u64,
}

impl WavSink {
    fn create(path: &Path) -> Result<WavSink> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).io_ctx("create dir", parent)?;
        }
        let writer = hound::WavWriter::create(path, wav_spec_16k()).map_err(audio_err)?;
        Ok(WavSink { writer, samples: 0 })
    }

    #[allow(clippy::cast_possible_truncation)] // clamped to [-1, 1] first
    fn write(&mut self, samples: &[f32]) -> Result<()> {
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
            self.writer.write_sample(v).map_err(audio_err)?;
        }
        self.samples += samples.len() as u64;
        Ok(())
    }

    /// Write the header; returns the duration in milliseconds.
    fn finish(self) -> Result<u64> {
        self.writer.finalize().map_err(audio_err)?;
        Ok(self.samples * 1000 / u64::from(TARGET_HZ))
    }
}

/// Write 16 kHz mono i16 PCM. Returns the duration in milliseconds.
pub fn write_wav_16k(path: &Path, samples: &[f32]) -> Result<u64> {
    let mut sink = WavSink::create(path)?;
    sink.write(samples)?;
    sink.finish()
}

// DSP over audio buffers: lengths are far below 2^23 samples per second of audio and
// sample values are in [-1, 1], so these float/int conversions cannot lose meaningful
// precision or wrap.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
/// Read any int/float WAV into mono 16 kHz f32 samples in [-1, 1].
pub fn read_wav_16k_f32(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).map_err(audio_err)?;
    let spec = reader.spec();
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()
            .map_err(audio_err)?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.saturating_sub(1))) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<std::result::Result<_, _>>()
                .map_err(audio_err)?
        }
    };
    let mono = to_mono(&raw, spec.channels);
    Ok(resample_linear(&mono, spec.sample_rate, TARGET_HZ))
}

/// A finished recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingInfo {
    /// Where the WAV was written.
    pub path: PathBuf,
    /// Length of the 16 kHz audio.
    pub duration_ms: u64,
}

enum Cmd {
    Start {
        path: PathBuf,
        reply: Sender<Result<()>>,
    },
    Stop {
        reply: Sender<Result<RecordingInfo>>,
    },
}

/// Runs on the recorder thread when a recording reaches its limit and has been
/// finalised; the result waits in [`RecorderHandle::take_auto_stopped`]. Must not block.
pub type AutoStopHook = Arc<dyn Fn() + Send + Sync>;

/// Handle to the recorder thread. Cheap to share; `Send + Sync`.
pub struct RecorderHandle {
    tx: Sender<Cmd>,
    recording: Arc<AtomicBool>,
    auto_stopped: Arc<Mutex<Option<Result<RecordingInfo>>>>,
}

/// Opens the capture source for one recording; the real one is [`start_capture`].
type CaptureFn = Box<dyn Fn(PathBuf) -> Result<Active> + Send>;

impl RecorderHandle {
    /// Start the worker thread that owns the cpal stream, with [`MAX_RECORDING`] as the limit.
    pub fn spawn() -> Result<RecorderHandle> {
        Self::spawn_with_limit(MAX_RECORDING, None)
    }

    /// Like `spawn`, with a custom limit; `on_auto_stop` runs when a recording hits it.
    pub fn spawn_with_limit(
        limit: Duration,
        on_auto_stop: Option<AutoStopHook>,
    ) -> Result<RecorderHandle> {
        Self::spawn_with(limit, on_auto_stop, Box::new(start_capture))
    }

    fn spawn_with(
        limit: Duration,
        on_auto_stop: Option<AutoStopHook>,
        capture: CaptureFn,
    ) -> Result<RecorderHandle> {
        let (tx, rx) = mpsc::channel();
        let recording = Arc::new(AtomicBool::new(false));
        let auto_stopped = Arc::new(Mutex::new(None));
        let worker = Worker {
            limit,
            on_auto_stop,
            capture,
            recording: recording.clone(),
            auto_stopped: auto_stopped.clone(),
        };
        std::thread::Builder::new()
            .name("stow-recorder".into())
            .spawn(move || worker.run(rx))
            .map_err(|e| StowError::Audio(format!("could not start recorder thread: {e}")))?;
        Ok(RecorderHandle {
            tx,
            recording,
            auto_stopped,
        })
    }

    fn send<T>(&self, make: impl FnOnce(Sender<Result<T>>) -> Cmd) -> Result<T> {
        let (reply_tx, reply_rx) = mpsc::channel();
        let dead = || StowError::Audio("recorder thread stopped".into());
        self.tx.send(make(reply_tx)).map_err(|_| dead())?;
        reply_rx.recv().map_err(|_| dead())?
    }

    /// Open the default input device and start writing the WAV at `path`.
    pub fn start(&self, path: PathBuf) -> Result<()> {
        self.send(|reply| Cmd::Start { path, reply })
    }

    /// Stop and finalise the WAV given to `start`.
    pub fn stop(&self) -> Result<RecordingInfo> {
        self.send(|reply| Cmd::Stop { reply })
    }

    /// True between a successful `start` and the next `stop` (or auto-stop).
    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    /// The recording the limit stopped, once. `Err` when finalising its WAV failed.
    pub fn take_auto_stopped(&self) -> Option<Result<RecordingInfo>> {
        self.auto_stopped
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

/// Chunks buffered between the audio callback and the writer thread (~several seconds).
const PIPE_CHUNKS: usize = 1024;

/// Producer half of the capture pipe, owned by the realtime callback. `push` never
/// blocks or locks; a full pipe drops the chunk and counts it.
struct SampleSink {
    tx: SyncSender<Vec<f32>>,
    dropped: Arc<AtomicU64>,
}

impl SampleSink {
    fn push(&self, chunk: Vec<f32>) {
        if self.tx.try_send(chunk).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// A sink plus the writer thread that converts everything pushed (interleaved
/// `channels` at `sample_rate`) to 16 kHz mono and appends it to the WAV at `path`
/// until every sink is dropped; `join` then finalises the file and gives the duration
/// in milliseconds. The WAV is created here, so a bad path fails the start.
fn wav_pipe(
    capacity: usize,
    path: &Path,
    sample_rate: u32,
    channels: u16,
) -> Result<(SampleSink, Arc<AtomicU64>, JoinHandle<Result<u64>>)> {
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(capacity);
    let mut sink = WavSink::create(path)?;
    let writer = std::thread::Builder::new()
        .name("stow-recorder-writer".into())
        .spawn(move || {
            let mut resampler = Resampler::new(sample_rate, TARGET_HZ);
            let mut out = Vec::new();
            for chunk in rx {
                out.clear();
                resampler.push(&to_mono(&chunk, channels), &mut out);
                sink.write(&out)?;
            }
            out.clear();
            resampler.finish(&mut out);
            sink.write(&out)?;
            sink.finish()
        })
        .map_err(|e| StowError::Audio(format!("could not start writer thread: {e}")))?;
    let dropped = Arc::new(AtomicU64::new(0));
    Ok((
        SampleSink {
            tx,
            dropped: dropped.clone(),
        },
        dropped,
        writer,
    ))
}

/// What feeds the pipe; dropping it drops the last sink, which ends the writer.
#[allow(dead_code)] // held only for its Drop
enum Source {
    Cpal(cpal::Stream),
    #[cfg(test)]
    Fake(FakeSource),
}

/// Live capture state; lives only on the worker thread.
struct Active {
    source: Source,
    writer: JoinHandle<Result<u64>>,
    dropped: Arc<AtomicU64>,
    path: PathBuf,
    started: Instant,
}

struct Worker {
    limit: Duration,
    on_auto_stop: Option<AutoStopHook>,
    capture: CaptureFn,
    recording: Arc<AtomicBool>,
    auto_stopped: Arc<Mutex<Option<Result<RecordingInfo>>>>,
}

impl Worker {
    fn run(self, rx: Receiver<Cmd>) {
        let mut active: Option<Active> = None;
        loop {
            let cmd = match &active {
                None => match rx.recv() {
                    Ok(cmd) => cmd,
                    Err(_) => return,
                },
                Some(a) => match rx.recv_timeout(self.limit.saturating_sub(a.started.elapsed())) {
                    Ok(cmd) => cmd,
                    Err(RecvTimeoutError::Timeout) => {
                        self.auto_stop(&mut active);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        // The handle is gone mid-recording: finalise the WAV rather than
                        // leave a headerless file behind.
                        if let Some(a) = active.take() {
                            self.recording.store(false, Ordering::SeqCst);
                            if let Err(e) = finish_capture(a) {
                                tracing::warn!("recording not finalised: {e}");
                            }
                        }
                        return;
                    }
                },
            };
            match cmd {
                Cmd::Start { path, reply } => {
                    let res = if active.is_some() {
                        Err(StowError::Audio("already recording".into()))
                    } else {
                        (self.capture)(path).map(|a| {
                            active = Some(a);
                            self.recording.store(true, Ordering::SeqCst);
                        })
                    };
                    if reply.send(res).is_err() {
                        tracing::debug!("recorder start reply dropped: caller went away");
                    }
                }
                Cmd::Stop { reply } => {
                    let res = match active.take() {
                        None => Err(StowError::Audio("not recording".into())),
                        Some(a) => {
                            self.recording.store(false, Ordering::SeqCst);
                            finish_capture(a)
                        }
                    };
                    if reply.send(res).is_err() {
                        tracing::debug!("recorder stop reply dropped: caller went away");
                    }
                }
            }
        }
    }

    fn auto_stop(&self, active: &mut Option<Active>) {
        let Some(a) = active.take() else {
            return;
        };
        tracing::info!(limit_secs = self.limit.as_secs(), "recording hit its limit");
        self.recording.store(false, Ordering::SeqCst);
        let res = finish_capture(a);
        *self
            .auto_stopped
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(res);
        if let Some(hook) = &self.on_auto_stop {
            hook();
        }
    }
}

fn start_capture(path: PathBuf) -> Result<Active> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| StowError::Audio("no input device".into()))?;
    let config = device.default_input_config().map_err(audio_err)?;
    let (sink, dropped, writer) =
        wav_pipe(PIPE_CHUNKS, &path, config.sample_rate(), config.channels())?;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let stream = match sample_format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &stream_config, sink)?,
        cpal::SampleFormat::I16 => build::<i16>(&device, &stream_config, sink)?,
        cpal::SampleFormat::U16 => build::<u16>(&device, &stream_config, sink)?,
        cpal::SampleFormat::I32 => build::<i32>(&device, &stream_config, sink)?,
        other => {
            return Err(StowError::Audio(format!(
                "unsupported sample format {other}"
            )));
        }
    };
    stream.play().map_err(audio_err)?;
    Ok(Active {
        source: Source::Cpal(stream),
        writer,
        dropped,
        path,
        started: Instant::now(),
    })
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sink: SampleSink,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            *config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                sink.push(data.iter().map(|&s| s.to_sample::<f32>()).collect());
            },
            |err| tracing::warn!("audio input stream error: {err}"),
            None,
        )
        .map_err(audio_err)
}

fn finish_capture(a: Active) -> Result<RecordingInfo> {
    let Active {
        source,
        writer,
        dropped,
        path,
        started: _,
    } = a;
    // Dropping the source drops the callback and its sink, which ends the writer loop.
    drop(source);
    let duration_ms = writer
        .join()
        .map_err(|_| audio_err("sample writer panicked"))??;
    let lost = dropped.load(Ordering::Relaxed);
    if lost > 0 {
        tracing::warn!(chunks = lost, "audio pipe overflowed; samples were dropped");
    }
    Ok(RecordingInfo { path, duration_ms })
}

/// A producer thread that pushes 10 ms of 16 kHz mono every 10 ms until dropped.
#[cfg(test)]
struct FakeSource {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(test)]
impl Drop for FakeSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
fn start_fake_capture(path: PathBuf) -> Result<Active> {
    let (sink, dropped, writer) = wav_pipe(PIPE_CHUNKS, &path, TARGET_HZ, 1)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            sink.push(vec![0.1; 160]);
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    Ok(Active {
        source: Source::Fake(FakeSource {
            stop,
            thread: Some(thread),
        }),
        writer,
        dropped,
        path,
        started: Instant::now(),
    })
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;

    impl RecorderHandle {
        /// A recorder over [`start_fake_capture`]: no device needed.
        fn spawn_fake(limit: Duration, on_auto_stop: Option<AutoStopHook>) -> RecorderHandle {
            Self::spawn_with(limit, on_auto_stop, Box::new(start_fake_capture)).unwrap()
        }
    }

    #[test]
    fn resample_halves_length_48k_to_24k() {
        let input: Vec<f32> = (0..48_000).map(|i| (i as f32 / 48_000.0).sin()).collect();
        let out = resample_linear(&input, 48_000, 24_000);
        assert_eq!(out.len(), 24_000);
        // Linear interpolation of a smooth signal stays close to the source.
        assert!((out[12_000] - input[24_000]).abs() < 1e-3);
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let input = vec![0.1f32, -0.2, 0.3, 0.4];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }

    #[test]
    fn to_mono_averages_channels() {
        let frames = [1.0f32, 0.0, 0.5, -0.5, -1.0, 1.0];
        assert_eq!(to_mono(&frames, 2), vec![0.5, 0.0, 0.0]);
        assert_eq!(to_mono(&frames, 1), frames.to_vec());
    }

    #[test]
    fn wav_roundtrip_and_duration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let samples: Vec<f32> = (0..16_000).map(|i| (i as f32 * 0.01).sin() * 0.8).collect();
        let ms = write_wav_16k(&path, &samples).unwrap();
        assert_eq!(ms, 1000);
        let back = read_wav_16k_f32(&path).unwrap();
        assert_eq!(back.len(), samples.len());
        for (a, b) in back.iter().zip(&samples) {
            assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        }
    }

    #[test]
    fn write_wav_clamps_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.wav");
        write_wav_16k(&path, &[2.0, -2.0, 0.0]).unwrap();
        let back = read_wav_16k_f32(&path).unwrap();
        assert!((back[0] - 1.0).abs() < 1e-3);
        assert!((back[1] + 1.0).abs() < 1e-3);
    }

    #[test]
    fn read_wav_converts_stereo_44k_to_mono_16k() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..44_100 {
            w.write_sample(0.5f32).unwrap();
            w.write_sample(-0.5f32).unwrap();
        }
        w.finalize().unwrap();
        let back = read_wav_16k_f32(&path).unwrap();
        assert_eq!(back.len(), 16_000);
        assert!(back.iter().all(|v| v.abs() < 1e-6));
    }

    #[test]
    fn recorder_handle_idle_behaviour() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RecorderHandle>();
        let rec = RecorderHandle::spawn().unwrap();
        assert!(!rec.is_recording());
        assert!(matches!(rec.stop(), Err(StowError::Audio(_))));
        assert!(rec.take_auto_stopped().is_none());
    }

    /// Push `chunk` the way a slow writer would drain (the real callback just drops).
    fn push_blocking(sink: &SampleSink, chunk: Vec<f32>) {
        let mut chunk = chunk;
        loop {
            match sink.tx.try_send(chunk) {
                Ok(()) => return,
                Err(mpsc::TrySendError::Full(c) | mpsc::TrySendError::Disconnected(c)) => {
                    chunk = c;
                    std::thread::yield_now();
                }
            }
        }
    }

    #[test]
    fn sample_pipe_collects_chunks_in_order_across_threads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipe.wav");
        let (sink, dropped, writer) = wav_pipe(8, &path, TARGET_HZ, 1).unwrap();
        let producer = std::thread::spawn(move || {
            for i in 0..100 {
                push_blocking(&sink, vec![i as f32 / 100.0; 4]);
            }
        });
        producer.join().unwrap();
        assert_eq!(writer.join().unwrap().unwrap(), 400 * 1000 / 16_000);
        let all = read_wav_16k_f32(&path).unwrap();
        assert_eq!(all.len(), 400);
        assert!(all[0].abs() < 1e-3);
        assert!((all[399] - 0.99).abs() < 1e-3, "{}", all[399]);
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn sample_sink_drops_instead_of_blocking_when_full() {
        let (tx, _rx) = mpsc::sync_channel::<Vec<f32>>(1);
        let dropped = Arc::new(AtomicU64::new(0));
        let sink = SampleSink {
            tx,
            dropped: dropped.clone(),
        };
        sink.push(vec![1.0]);
        sink.push(vec![2.0]);
        sink.push(vec![3.0]);
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
    }

    fn sine(hz: f32, rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
    }

    #[test]
    fn downsample_attenuates_frequencies_above_target_nyquist() {
        // 20 kHz is above the 8 kHz Nyquist of 16 kHz output; without a low-pass it
        // would alias to 4 kHz at nearly full amplitude.
        let input = sine(20_000.0, 48_000, 1.0);
        let out = resample_linear(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        let ratio = rms(&out) / rms(&input);
        assert!(ratio < 0.25, "aliased content ratio {ratio}");
    }

    #[test]
    fn downsample_preserves_low_frequency() {
        let input = sine(300.0, 48_000, 1.0);
        let out = resample_linear(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        let ratio = rms(&out) / rms(&input);
        assert!((ratio - 1.0).abs() < 0.10, "300 Hz ratio {ratio}");
    }

    /// A signal with content across the band, so interpolation and the low-pass matter.
    fn busy_signal(rate: u32, secs: f32) -> Vec<f32> {
        let a = sine(440.0, rate, secs);
        let b = sine(3_100.0, rate, secs);
        let c = sine(11_000.0, rate, secs);
        a.iter()
            .zip(&b)
            .zip(&c)
            .map(|((a, b), c)| 0.5 * a + 0.3 * b + 0.2 * c)
            .collect()
    }

    /// Feed `input` through a fresh [`Resampler`] in chunks of irregular sizes.
    fn stream_in_chunks(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
        let mut rs = Resampler::new(from_hz, to_hz);
        let mut out = Vec::new();
        let sizes = [37usize, 480, 1, 1000, 3, 256];
        let mut i = 0;
        let mut k = 0;
        while i < input.len() {
            let end = (i + sizes[k % sizes.len()]).min(input.len());
            rs.push(&input[i..end], &mut out);
            i = end;
            k += 1;
        }
        rs.finish(&mut out);
        out
    }

    fn assert_close(stream: &[f32], batch: &[f32], tol: f32) {
        assert_eq!(stream.len(), batch.len(), "lengths");
        let worst = stream
            .iter()
            .zip(batch)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < tol, "max diff {worst}");
    }

    #[test]
    fn resampler_matches_batch_for_down_up_and_identity() {
        // Lengths that are not a multiple of the ratio, so `len * to / from` truncates.
        let down = &busy_signal(48_000, 0.5)[..23_998];
        assert_close(
            &stream_in_chunks(down, 48_000, 16_000),
            &resample_linear(down, 48_000, 16_000),
            1e-5,
        );
        // 44.1 kHz: a non-integer ratio, so the fractional position carries across chunks.
        let odd = &busy_signal(44_100, 0.5)[..22_043];
        assert_close(
            &stream_in_chunks(odd, 44_100, 16_000),
            &resample_linear(odd, 44_100, 16_000),
            1e-5,
        );
        let up = busy_signal(8_000, 0.5);
        assert_close(
            &stream_in_chunks(&up, 8_000, 16_000),
            &resample_linear(&up, 8_000, 16_000),
            1e-6,
        );
        let same = busy_signal(16_000, 0.1);
        assert_eq!(stream_in_chunks(&same, 16_000, 16_000), same);
        // Tiny inputs (shorter than the low-pass window) still produce `len * to / from`.
        assert_close(
            &stream_in_chunks(&[0.5, -0.5], 48_000, 16_000),
            &resample_linear(&[0.5, -0.5], 48_000, 16_000),
            1e-6,
        );
        assert!(stream_in_chunks(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn streaming_writer_matches_batch_resample_within_tolerance() {
        let dir = tempfile::tempdir().unwrap();
        // 48 kHz stereo with different content per channel, as a device delivers it.
        let left = busy_signal(48_000, 0.5);
        let right = sine(1_000.0, 48_000, 0.5);
        let frames: Vec<f32> = left
            .iter()
            .zip(&right)
            .flat_map(|(l, r)| [*l, *r * 0.7])
            .collect();

        let batch_path = dir.path().join("batch.wav");
        let batch_ms = write_wav_16k(
            &batch_path,
            &resample_linear(&to_mono(&frames, 2), 48_000, TARGET_HZ),
        )
        .unwrap();

        let stream_path = dir.path().join("stream.wav");
        let (sink, dropped, writer) = wav_pipe(4, &stream_path, 48_000, 2).unwrap();
        let producer = std::thread::spawn(move || {
            // Whole frames per chunk, irregular sizes.
            let sizes = [2usize * 37, 2 * 480, 2, 2 * 1000, 2 * 3];
            let mut i = 0;
            let mut k = 0;
            while i < frames.len() {
                let end = (i + sizes[k % sizes.len()]).min(frames.len());
                push_blocking(&sink, frames[i..end].to_vec());
                i = end;
                k += 1;
            }
        });
        producer.join().unwrap();
        let stream_ms = writer.join().unwrap().unwrap();
        assert_eq!(stream_ms, batch_ms);
        assert_eq!(stream_ms, 500);
        assert_eq!(dropped.load(Ordering::Relaxed), 0);

        let batch = read_wav_16k_f32(&batch_path).unwrap();
        let stream = read_wav_16k_f32(&stream_path).unwrap();
        // Both are i16-quantised; one LSB is ~3e-5.
        assert_close(&stream, &batch, 1e-3);
        assert!(rms(&stream) > 0.1, "not silence");
    }

    #[test]
    fn auto_stop_fires_after_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auto.wav");
        let (fired_tx, fired_rx) = mpsc::channel::<()>();
        let hook: AutoStopHook = Arc::new(move || {
            let _ = fired_tx.send(());
        });
        let rec = RecorderHandle::spawn_fake(Duration::from_millis(150), Some(hook));
        rec.start(path.clone()).unwrap();
        assert!(rec.is_recording());
        assert!(
            rec.take_auto_stopped().is_none(),
            "nothing before the limit"
        );

        fired_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("hook runs once the limit passes");
        assert!(!rec.is_recording());
        let info = rec.take_auto_stopped().expect("result is waiting").unwrap();
        assert_eq!(info.path, path);
        assert!(
            (50..=1000).contains(&info.duration_ms),
            "about 150 ms of audio, got {}",
            info.duration_ms
        );
        let samples = read_wav_16k_f32(&path).unwrap();
        assert_eq!(
            samples.len() as u64 / 16,
            info.duration_ms,
            "header finalised"
        );
        assert!(samples.iter().all(|s| (s - 0.1).abs() < 1e-3));

        assert!(rec.take_auto_stopped().is_none(), "taken once");
        assert!(
            matches!(rec.stop(), Err(StowError::Audio(_))),
            "already stopped"
        );
        // The recorder is reusable afterwards.
        rec.start(dir.path().join("second.wav")).unwrap();
        let second = rec.stop().unwrap();
        assert!(second.path.is_file());
    }

    #[test]
    fn stop_before_limit_leaves_no_auto_stop_result() {
        let dir = tempfile::tempdir().unwrap();
        let rec = RecorderHandle::spawn_fake(Duration::from_secs(60), None);
        rec.start(dir.path().join("manual.wav")).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        let info = rec.stop().unwrap();
        assert!(info.path.is_file());
        assert!(!rec.is_recording());
        assert!(rec.take_auto_stopped().is_none());
    }
}
