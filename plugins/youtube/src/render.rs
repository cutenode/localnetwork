//! The video YouTube takes for a set: a still picture (the `picture` setting, else a black frame)
//! shown for as long as the recording plays, over its audio, made by FFmpeg from `PATH`. ffprobe
//! says how long the recording is and what its audio is; ffmpeg makes an MP4 of one frame a second
//! in H.264 (`-tune stillimage`: a picture that doesn't move costs next to nothing), its audio AAC,
//! copied when it's AAC already, else encoded at the 48 kHz and 384 kbit/s YouTube recommends for
//! stereo, and its index at the front (`+faststart`) with no edit lists, as YouTube asks. Without
//! an edit list to shift it back, a B-frame's reordering would start the picture two seconds late
//! (two frames, at a frame a second), so it has none.
//!
//! The picture is cut to the recording's length (`-t` on its input) rather than ended with
//! `-shortest`: at a frame a second the encoder holds back a minute of frames or so, and
//! `-shortest` ran a 25-second recording's video to 96 seconds.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use loclib_plugin::RpcError;
use loclib_progress::{Ctx, Event, Phase};
use serde::Deserialize;

/// What YouTube recommends for stereo audio, in kbit/s.
const AAC_KBPS: u32 = 384;
/// The frame: 1080p, which a picture is scaled down to fit and centred in.
const FRAME: &str = "1920:1080";
/// How often making the video notices the hook being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);

/// A recording's audio, as ffprobe reads it.
#[derive(Debug)]
pub(crate) struct Audio {
    /// FFmpeg's name for its codec (`aac`, `mp3`, `pcm_s16le`...).
    pub(crate) codec: String,
    pub(crate) seconds: f64,
}

/// What ffprobe answers, as far as it's read.
#[derive(Deserialize)]
struct Probed {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    codec_name: String,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

/// `command` started, its output piped, or why it couldn't be: not installed, said so.
fn start(mut command: Command) -> Result<Child, RpcError> {
    let program = command.get_program().to_string_lossy().into_owned();
    let child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    child.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => RpcError::failed(
            "not_found",
            format!(
                "{program} isn't installed: YouTube takes a video, which FFmpeg makes from the recording. Install it \
                 (https://ffmpeg.org/download.html) and publish again"
            ),
        ),
        _ => RpcError::failed("external", format!("{program} couldn't be started: {e}")),
    })
}

/// The last line `program` said on standard error, as its failure: why it failed.
fn failed(program: &str, status: std::process::ExitStatus, said: &str) -> RpcError {
    let why = said.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("it said nothing more");
    RpcError::failed("external", format!("{program} failed ({status}): {why}"))
}

/// How long `recording` is and what its audio is, as ffprobe reads it.
pub(crate) fn probe(recording: &Path) -> Result<Audio, RpcError> {
    let mut ffprobe = Command::new("ffprobe");
    ffprobe.args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_name:format=duration"]);
    ffprobe.args(["-of", "json"]).arg(recording);
    let out = start(ffprobe)?.wait_with_output().map_err(|e| RpcError::failed("external", format!("ffprobe: {e}")))?;
    if !out.status.success() {
        return Err(failed("ffprobe", out.status, &String::from_utf8_lossy(&out.stderr)));
    }
    let name = recording.file_name().unwrap_or_default().to_string_lossy();
    let probed: Probed = serde_json::from_slice(&out.stdout)
        .map_err(|e| RpcError::failed("external", format!("ffprobe's answer can't be read: {e}")))?;
    let Some(Stream { codec_name: codec }) = probed.streams.into_iter().next() else {
        return Err(RpcError::failed("invalid", format!("{name} has no audio FFmpeg can read")));
    };
    let seconds = probed.format.and_then(|f| f.duration).and_then(|d| d.parse::<f64>().ok());
    match seconds {
        Some(seconds) if seconds > 0.0 => Ok(Audio { codec, seconds }),
        _ => Err(RpcError::failed("invalid", format!("FFmpeg can't tell how long {name} is"))),
    }
}

/// Makes the video of `recording` (`audio`, `bytes` long) at `to`, showing `picture` (else a black
/// frame), reporting how much of the recording it has made it of (from ffmpeg's `-progress`) in
/// the write phase. Cancelled, ffmpeg is stopped.
pub(crate) fn render(
    recording: &Path,
    audio: &Audio,
    bytes: u64,
    picture: Option<&Path>,
    to: &Path,
    ctx: &Ctx<'_>,
) -> Result<(), RpcError> {
    let seconds = format!("{:.6}", audio.seconds);
    let black = format!("color=c=black:s={}:r=1", FRAME.replace(':', "x"));
    let fit =
        format!("scale={FRAME}:force_original_aspect_ratio=decrease,pad={FRAME}:(ow-iw)/2:(oh-ih)/2,format=yuv420p");
    let bitrate = format!("{AAC_KBPS}k");
    let sound = match audio.codec.as_str() {
        "aac" => ["-c:a", "copy"].as_slice(),
        _ => &["-c:a", "aac", "-b:a", &bitrate, "-ar", "48000"],
    };
    let mut ffmpeg = Command::new("ffmpeg");
    ffmpeg.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]);
    match picture {
        Some(picture) => ffmpeg.args(["-loop", "1", "-framerate", "1", "-t", &seconds, "-i"]).arg(picture),
        None => ffmpeg.args(["-f", "lavfi", "-t", &seconds, "-i", &black]),
    };
    ffmpeg.arg("-i").arg(recording);
    ffmpeg.args(["-map", "0:v", "-map", "1:a:0", "-vf", &fit, "-c:v", "libx264", "-tune", "stillimage", "-bf", "0"]);
    ffmpeg
        .args(sound)
        .args(["-use_editlist", "0", "-movflags", "+faststart", "-progress", "pipe:1", "-nostats"])
        .arg(to);
    let mut child = start(ffmpeg)?;
    let (stdout, mut stderr) = (child.stdout.take(), child.stderr.take());
    // Each read on its own, so neither pipe fills up, and a cancel is noticed while ffmpeg says
    // nothing.
    let said = std::thread::spawn(move || {
        let mut said = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut said);
        }
        said
    });
    let (lines, progress) = mpsc::channel();
    std::thread::spawn(move || {
        for line in stdout.map(BufReader::new).into_iter().flat_map(BufRead::lines).map_while(Result::ok) {
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let name = recording.file_name().unwrap_or_default().to_string_lossy().into_owned();
    ctx.phase_start(Phase::Write, 1, bytes);
    ctx.emit(Event::Item { phase: Phase::Write, path: name, bytes });
    let mut made = 0;
    loop {
        if ctx.check().is_err() {
            let _ = child.kill();
            break;
        }
        let line = match progress.recv_timeout(CHECK_EVERY) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let Some(us) = line.strip_prefix("out_time_us=").and_then(|n| n.trim().parse::<f64>().ok()) else { continue };
        // How much of the recording that far into it is.
        let now = (bytes as f64 * (us / 1e6 / audio.seconds).clamp(0.0, 1.0)) as u64;
        if now > made {
            ctx.emit(Event::Bytes { phase: Phase::Write, bytes: now - made });
            made = now;
        }
    }
    let status = child.wait().map_err(|e| RpcError::failed("external", format!("ffmpeg: {e}")));
    ctx.phase_end(Phase::Write);
    let said = said.join().unwrap_or_default();
    ctx.check()?;
    let status = status?;
    if !status.success() {
        return Err(failed("ffmpeg", status, &said));
    }
    Ok(())
}
