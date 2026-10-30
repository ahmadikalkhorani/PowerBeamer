#![allow(dead_code)]
use image::RgbaImage;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// Look for an executable in PATH or standard system paths
pub fn find_executable(name: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PATH") {
        for entry in std::env::split_paths(&path) {
            let candidate = entry.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for candidate in &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        let p = Path::new(candidate).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Probe video duration using ffprobe CLI
pub fn probe_video_duration(file_path: &str) -> f32 {
    if let Some(ffprobe) = find_executable("ffprobe") {
        let output = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                file_path,
            ])
            .output();
        if let Ok(out) = output {
            let str_val = String::from_utf8_lossy(&out.stdout);
            if let Ok(dur) = str_val.trim().parse::<f32>() {
                if dur > 0.0 {
                    return dur;
                }
            }
        }
    }
    6.0
}

/// Probe video width and height using ffprobe CLI
pub fn probe_video_dimensions(file_path: &str) -> Option<(u32, u32)> {
    if let Some(ffprobe) = find_executable("ffprobe") {
        let output = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=s=x:p=0",
                file_path,
            ])
            .output();
        if let Ok(out) = output {
            let str_val = String::from_utf8_lossy(&out.stdout);
            let parts: Vec<&str> = str_val.trim().split('x').collect();
            if parts.len() == 2 {
                if let (Ok(w), Ok(h)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    return Some((w, h));
                }
            }
        }
    }
    None
}

/// Two reusable decode surfaces. Slint's copy-on-write detaches a surface if a
/// slow renderer still holds it, so reuse never mutates a displayed frame.
pub struct VideoFrameBuffers {
    buffers: [slint::SharedPixelBuffer<slint::Rgba8Pixel>; 2],
    next: usize,
}
impl VideoFrameBuffers {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            buffers: [
                slint::SharedPixelBuffer::new(width, height),
                slint::SharedPixelBuffer::new(width, height),
            ],
            next: 0,
        }
    }
    pub fn next_mut(&mut self) -> &mut slint::SharedPixelBuffer<slint::Rgba8Pixel> {
        let index = self.next;
        self.next = (index + 1) % 2;
        &mut self.buffers[index]
    }
}

/// Native in-place video player that stream-decodes frames directly into Slint Image
/// without popping up any external windows.
pub struct InPlaceVideoPlayer {
    is_playing: Arc<AtomicBool>,
    ffmpeg_proc: Arc<Mutex<Option<Child>>>,
    audio_proc: Arc<Mutex<Option<Child>>>,
    worker_thread: Option<JoinHandle<()>>,
    current_time: Arc<AtomicU32>,
    generation: Arc<AtomicU64>,
}

impl InPlaceVideoPlayer {
    pub fn new() -> Self {
        Self {
            is_playing: Arc::new(AtomicBool::new(false)),
            ffmpeg_proc: Arc::new(Mutex::new(None)),
            audio_proc: Arc::new(Mutex::new(None)),
            worker_thread: None,
            current_time: Arc::new(AtomicU32::new(0)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing.load(Ordering::SeqCst)
    }

    pub fn current_time(&self) -> f32 {
        f32::from_bits(self.current_time.load(Ordering::SeqCst))
    }

    pub fn set_current_time(&self, time: f32) {
        self.current_time.store(time.to_bits(), Ordering::SeqCst);
    }

    pub fn generation(&self) -> (Arc<AtomicU64>, u64) {
        (
            self.generation.clone(),
            self.generation.load(Ordering::Acquire),
        )
    }

    pub fn stop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.is_playing.store(false, Ordering::SeqCst);
        self.current_time = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        if let Ok(mut lock) = self.ffmpeg_proc.lock() {
            if let Some(child) = lock.take() {
                kill_child(child);
            }
        }
        if let Ok(mut lock) = self.audio_proc.lock() {
            if let Some(child) = lock.take() {
                kill_child(child);
            }
        }
        if let Some(handle) = self.worker_thread.take() {
            thread::spawn(move || {
                let _ = handle.join();
            });
        }
    }

    /// Plays an audio-only clip in the background. The process is owned by this player, so
    /// `pause`/`stop` end exactly this process and nothing else on the system.
    pub fn play_audio(&mut self, file_path: &str, start_time_secs: f32) -> bool {
        self.play_audio_with_options(file_path, start_time_secs, 1.0, false)
    }

    pub fn play_audio_with_options(
        &mut self,
        file_path: &str,
        start_time_secs: f32,
        speed: f32,
        muted: bool,
    ) -> bool {
        self.pause();
        self.is_playing = Arc::new(AtomicBool::new(false));
        self.current_time = Arc::new(AtomicU32::new(start_time_secs.to_bits()));
        let base_dir = std::path::Path::new(file_path)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let child = if let Some(bin) = find_executable("ffplay") {
            Command::new(bin)
                .args([
                    "-nodisp",
                    "-vn",
                    "-autoexit",
                    "-loglevel",
                    "quiet",
                    "-infbuf",
                    "-fflags",
                    "nobuffer",
                    "-flags",
                    "low_delay",
                    "-volume",
                    if muted { "0" } else { "100" },
                    "-af",
                    &audio_tempo_filter(speed),
                    "-ss",
                    &format!("{:.2}", start_time_secs),
                    file_path,
                ])
                .current_dir(base_dir)
                .spawn()
                .ok()
        } else if start_time_secs <= 0.01 {
            find_executable("afplay").and_then(|bin| {
                Command::new(bin)
                    .args([
                        "-v",
                        if muted { "0" } else { "1" },
                        "-r",
                        &format!("{:.2}", speed.clamp(0.25, 4.0)),
                        file_path,
                    ])
                    .current_dir(base_dir)
                    .spawn()
                    .ok()
            })
        } else {
            None
        };
        let started = child.is_some();
        if let Ok(mut lock) = self.audio_proc.lock() {
            *lock = child;
        }
        self.is_playing.store(started, Ordering::SeqCst);
        started
    }

    pub fn pause(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.is_playing.store(false, Ordering::SeqCst);
        if let Ok(mut lock) = self.ffmpeg_proc.lock() {
            if let Some(child) = lock.take() {
                kill_child(child);
            }
        }
        if let Ok(mut lock) = self.audio_proc.lock() {
            if let Some(child) = lock.take() {
                kill_child(child);
            }
        }
        if let Some(handle) = self.worker_thread.take() {
            thread::spawn(move || {
                let _ = handle.join();
            });
        }
    }

    pub fn play<F, E>(
        &mut self,
        file_path: &str,
        start_time_secs: f32,
        speed: f32,
        duration: f32,
        muted: bool,
        on_frame: F,
        on_eof: E,
    ) -> bool
    where
        F: Fn(slint::SharedPixelBuffer<slint::Rgba8Pixel>, f32, f32) + Send + 'static,
        E: Fn() + Send + 'static,
    {
        self.pause();
        self.current_time = Arc::new(AtomicU32::new(start_time_secs.to_bits()));

        let ffmpeg_bin = match find_executable("ffmpeg") {
            Some(bin) => bin,
            None => {
                eprintln!("ffmpeg binary not found; in-place video playback unavailable");
                return false;
            }
        };

        self.is_playing = Arc::new(AtomicBool::new(true));
        let is_running = self.is_playing.clone();
        is_running.store(true, Ordering::SeqCst);

        // 1. Audio playback in background without window (only if not muted)
        let audio_child = if !muted {
            if let Some(ffplay) = find_executable("ffplay") {
                let mut cmd = Command::new(ffplay);
                let mut args = vec![
                    "-nodisp".to_string(),
                    "-vn".to_string(),
                    "-autoexit".to_string(),
                    "-loglevel".to_string(),
                    "quiet".to_string(),
                    "-ss".to_string(),
                    format!("{:.2}", start_time_secs),
                ];
                if (speed - 1.0).abs() > 0.05 {
                    args.push("-af".to_string());
                    args.push(audio_tempo_filter(speed));
                }
                args.push(file_path.to_string());
                cmd.args(&args);
                cmd.spawn().ok()
            } else if let Some(afplay) = find_executable("afplay") {
                let mut cmd = Command::new(afplay);
                let remaining = (duration - start_time_secs).max(0.1);
                cmd.args([
                    "-t",
                    &format!("{:.2}", remaining),
                    "-r",
                    &format!("{:.2}", speed),
                    file_path,
                ]);
                cmd.spawn().ok()
            } else {
                None
            }
        } else {
            None
        };

        if let Ok(mut lock) = self.audio_proc.lock() {
            *lock = audio_child;
        }

        // 2. Stream-decode frames at 30 fps
        let out_w = 640u32;
        let out_h = 360u32;
        // Output a fixed 30 displayed frames per second. PTS chooses source time;
        // the worker's wall clock paces delivery once, independently of speed.
        let pts_scale = 1.0 / speed.clamp(0.25, 4.0);
        let vf_arg = format!(
            "setpts={:.6}*PTS,fps=30,scale={}:{}:flags=bilinear",
            pts_scale, out_w, out_h
        );

        let mut ffmpeg_cmd = Command::new(ffmpeg_bin);
        ffmpeg_cmd.args([
            "-loglevel",
            "quiet",
            "-ss",
            &format!("{:.2}", start_time_secs),
            "-i",
            file_path,
            "-vf",
            &vf_arg,
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ]);
        ffmpeg_cmd.stdout(Stdio::piped());
        ffmpeg_cmd.stderr(Stdio::null());

        let mut child = match ffmpeg_cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to spawn ffmpeg: {}", e);
                is_running.store(false, Ordering::SeqCst);
                return false;
            }
        };

        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                is_running.store(false, Ordering::SeqCst);
                return false;
            }
        };

        if let Ok(mut lock) = self.ffmpeg_proc.lock() {
            *lock = Some(child);
        }

        let is_running_clone = is_running.clone();
        let current_time_clone = self.current_time.clone();

        let handle = thread::spawn(move || {
            let mut reader = stdout;
            let mut cur_time = start_time_secs;
            let clamped_speed = speed.clamp(0.25, 4.0);
            let frame_dur = std::time::Duration::from_secs_f64(1.0 / 30.0);
            let mut next_frame_instant = std::time::Instant::now();
            let dt = (1.0 / 30.0) * clamped_speed;
            let mut buffers = VideoFrameBuffers::new(out_w, out_h);

            while is_running_clone.load(Ordering::SeqCst) {
                let pixel_buf = buffers.next_mut();
                match reader.read_exact(pixel_buf.make_mut_bytes()) {
                    Ok(()) => {
                        if !is_running_clone.load(Ordering::Acquire) {
                            break;
                        }
                        let now = std::time::Instant::now();
                        if now < next_frame_instant {
                            thread::sleep(next_frame_instant - now);
                        }
                        if !is_running_clone.load(Ordering::Acquire) {
                            break;
                        }

                        cur_time += dt;
                        current_time_clone.store(cur_time.to_bits(), Ordering::SeqCst);
                        let progress = if duration > 0.0 {
                            (cur_time / duration).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };

                        on_frame(pixel_buf.clone(), cur_time, progress);

                        next_frame_instant = std::cmp::max(
                            std::time::Instant::now(),
                            next_frame_instant + frame_dur,
                        );
                    }
                    Err(_) => {
                        break;
                    }
                }
            }

            if is_running_clone.load(Ordering::SeqCst) {
                is_running_clone.store(false, Ordering::SeqCst);
                current_time_clone.store(duration.to_bits(), Ordering::SeqCst);
                on_eof();
            }
        });

        self.worker_thread = Some(handle);
        true
    }

    pub fn extract_single_frame(file_path: &str, time_secs: f32) -> Option<RgbaImage> {
        let ffmpeg_bin = find_executable("ffmpeg")?;
        let out_w = 640u32;
        let out_h = 360u32;
        let vf_arg = format!("scale={}:{}:flags=bilinear", out_w, out_h);

        let output = Command::new(ffmpeg_bin)
            .args([
                "-loglevel",
                "quiet",
                "-ss",
                &format!("{:.2}", time_secs),
                "-i",
                file_path,
                "-vf",
                &vf_arg,
                "-vframes",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "pipe:1",
            ])
            .output()
            .ok()?;

        let frame_size = (out_w * out_h * 4) as usize;
        if output.stdout.len() >= frame_size {
            let mut pixels = output.stdout;
            pixels.truncate(frame_size);
            image::RgbaImage::from_raw(out_w, out_h, pixels)
        } else {
            None
        }
    }
}

impl Drop for InPlaceVideoPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Kills a child process and reaps it on a background thread so no zombie is left behind
/// and the caller never blocks.
fn kill_child(mut child: Child) {
    let _ = child.kill();
    thread::spawn(move || {
        let _ = child.wait();
    });
}

fn audio_tempo_filter(speed: f32) -> String {
    let mut speed = speed.clamp(0.25, 4.0);
    let mut filters = Vec::new();
    while speed < 0.5 {
        filters.push("atempo=0.5".to_string());
        speed /= 0.5;
    }
    while speed > 2.0 {
        filters.push("atempo=2.0".to_string());
        speed /= 2.0;
    }
    filters.push(format!("atempo={speed:.4}"));
    filters.join(",")
}

#[cfg(test)]
mod control_tests {
    #[test]
    fn audio_speed_filter_preserves_every_supported_speed() {
        for speed in [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0] {
            let product: f32 = super::audio_tempo_filter(speed)
                .split(',')
                .map(|s| {
                    let factor = s.strip_prefix("atempo=").unwrap().parse::<f32>().unwrap();
                    assert!((0.5..=2.0).contains(&factor));
                    factor
                })
                .product();
            assert!((product - speed).abs() < 0.0001);
        }
    }
}
