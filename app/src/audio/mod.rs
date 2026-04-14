use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use tempfile::{NamedTempFile, TempPath};

const FFMPEG_STDERR_CAP_BYTES: usize = 64 * 1024;

pub fn try_default_output() -> Result<(rodio::OutputStream, rodio::OutputStreamHandle), String> {
    rodio::OutputStream::try_default()
        .map_err(|_| "audio output unavailable (no default output device?)".to_string())
}

type BoxedReadSeek = Box<dyn Read + Seek + Send>;

pub fn decode_file(path: &Path) -> Result<rodio::Decoder<BufReader<BoxedReadSeek>>, String> {
    if !is_ogg(path) {
        return decode_file_default(path);
    }

    decode_ogg_with_fallback(
        path,
        || decode_file_default(path),
        || decode_file_via_ffmpeg(path),
    )
}

fn decode_file_default(path: &Path) -> Result<rodio::Decoder<BufReader<BoxedReadSeek>>, String> {
    let f = File::open(path).map_err(|e| format!("failed to open `{}`: {e}", path.display()))?;
    let reader: BoxedReadSeek = Box::new(f);
    let reader = BufReader::new(reader);
    rodio::Decoder::new(reader).map_err(|e| format!("failed to decode `{}`: {e}", path.display()))
}

fn decode_file_via_ffmpeg(path: &Path) -> Result<rodio::Decoder<BufReader<BoxedReadSeek>>, String> {
    let (ffmpeg, searched) = find_ffmpeg()?;

    // Convert to WAV on stdout, then let rodio decode WAV.
    // Spool stdout to a temp file so we don't buffer full WAV in memory (rodio needs Read+Seek).
    let mut tmp = NamedTempFile::new()
        .map_err(|e| format!("failed to create temp file for ffmpeg WAV output: {e}"))?;
    let stdout_file = tmp
        .as_file()
        .try_clone()
        .map_err(|e| format!("failed to clone temp file handle for ffmpeg WAV output: {e}"))?;

    let mut child = Command::new(&ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-f", "wav", "pipe:1"])
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "failed to run ffmpeg fallback for `.ogg` `{}`: {e}\nSearched locations:\n{}",
                path.display(),
                format_searched_locations(&searched)
            )
        })?;

    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture ffmpeg stderr".to_string())?;
    let stderr_reader = thread::spawn(move || {
        read_to_end_capped(&mut stderr_pipe, FFMPEG_STDERR_CAP_BYTES)
    });

    let status = child
        .wait()
        .map_err(|e| format!("failed while waiting for ffmpeg fallback process: {e}"))?;
    let stderr_bytes = join_ffmpeg_stderr_reader(stderr_reader.join());

    if !status.success() {
        let stderr = trim_ffmpeg_stderr(&stderr_bytes);
        return Err(format!(
            "ffmpeg fallback failed for `.ogg` `{}` (exit {}).\n\
ffmpeg: `{}`\n\
stderr (trimmed):\n{stderr}",
            path.display(),
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            ffmpeg.display(),
        ));
    }

    let temp_path = tmp.into_temp_path();
    let file = File::open(&temp_path)
        .map_err(|e| format!("failed to open ffmpeg WAV temp file for `{}`: {e}", path.display()))?;
    let reader: BoxedReadSeek = Box::new(TempFileReadSeek::new(file, temp_path));
    let reader = BufReader::new(reader);
    rodio::Decoder::new(reader).map_err(|e| {
        format!(
            "ffmpeg produced WAV but decoding still failed for `{}`: {e}",
            path.display()
        )
    })
}

fn join_ffmpeg_stderr_reader(join_result: std::thread::Result<Vec<u8>>) -> Vec<u8> {
    match join_result {
        Ok(v) => v,
        Err(_) => b"<ffmpeg stderr reader panicked>".to_vec(),
    }
}

struct TempFileReadSeek {
    file: File,
    // Keeps the temp file path alive until the decoder is dropped.
    _path: TempPath,
}

impl TempFileReadSeek {
    fn new(file: File, path: TempPath) -> Self {
        Self { file, _path: path }
    }
}

impl Read for TempFileReadSeek {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buf)
    }
}

impl Seek for TempFileReadSeek {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.file.seek(pos)
    }
}

fn decode_ogg_with_fallback<T>(
    path: &Path,
    default_decode: impl FnOnce() -> Result<T, String>,
    ffmpeg_fallback: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    match default_decode() {
        Ok(v) => Ok(v),
        Err(default_err) => match ffmpeg_fallback() {
            Ok(v) => Ok(v),
            Err(ffmpeg_err) => Err(format!(
                "failed to decode `.ogg` `{}`. Tried built-in decoder first, then ffmpeg fallback.\n\
Built-in decode error: {default_err}\n\
FFmpeg fallback error: {ffmpeg_err}",
                path.display()
            )),
        },
    }
}

fn is_ogg(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };
    ext.eq_ignore_ascii_case("ogg")
}

fn find_ffmpeg() -> Result<(PathBuf, Vec<String>), String> {
    let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let path_env = std::env::var_os("PATH");
    find_ffmpeg_from(exe_name, exe_dir.as_deref(), path_env.as_deref())
}

fn find_ffmpeg_from(
    exe_name: &str,
    exe_dir: Option<&Path>,
    path_env: Option<&OsStr>,
) -> Result<(PathBuf, Vec<String>), String> {
    let mut searched: Vec<String> = Vec::new();

    let tools_rel = Path::new("data").join("tools").join(exe_name);

    if let Some(dir) = exe_dir {
        let near_binary = dir.join(exe_name);
        searched.push(format!("near binary: `{}`", near_binary.display()));
        if near_binary.is_file() {
            return Ok((near_binary, searched));
        }

        let tools_under_exe = dir.join(&tools_rel);
        searched.push(format!(
            "near binary tools: `{}`",
            tools_under_exe.display()
        ));
        if tools_under_exe.is_file() {
            return Ok((tools_under_exe, searched));
        }

        if let Some(parent) = dir.parent() {
            let tools_next_to_exe = parent.join(&tools_rel);
            searched.push(format!(
                "sibling tools: `{}`",
                tools_next_to_exe.display()
            ));
            if tools_next_to_exe.is_file() {
                return Ok((tools_next_to_exe, searched));
            }
        }
    } else {
        searched.push("near binary: <unavailable: current_exe()>".to_string());
    }

    if let Some(found) = find_in_path(exe_name, path_env, &mut searched) {
        return Ok((found, searched));
    }

    Err(format!(
        "ffmpeg not found.\nSearched locations:\n{}",
        format_searched_locations(&searched)
    ))
}

fn find_in_path(exe_name: &str, path_env: Option<&OsStr>, searched: &mut Vec<String>) -> Option<PathBuf> {
    let path_env = path_env?;
    searched.push(format!("PATH lookup for `{exe_name}`"));

    let paths = std::env::split_paths(path_env);
    for dir in paths {
        let candidate = dir.join(exe_name);
        searched.push(format!("PATH dir: `{}`", candidate.display()));
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn format_searched_locations(searched: &[String]) -> String {
    searched
        .iter()
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_to_end_capped(reader: &mut impl Read, cap_bytes: usize) -> Vec<u8> {
    use std::collections::VecDeque;

    if cap_bytes == 0 {
        // Drain reader but keep nothing.
        let mut tmp = [0u8; 8192];
        while reader.read(&mut tmp).unwrap_or(0) != 0 {}
        return Vec::new();
    }

    let mut out: VecDeque<u8> = VecDeque::with_capacity(cap_bytes.min(8192));
    let mut buf = [0u8; 8192];

    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        out.extend(&buf[..n]);
        if out.len() > cap_bytes {
            let excess = out.len() - cap_bytes;
            out.drain(..excess);
        }
    }

    out.into_iter().collect()
}

fn trim_ffmpeg_stderr(stderr: &[u8]) -> String {
    const MAX_CHARS: usize = 2000;
    const MAX_LINES: usize = 30;

    let s = String::from_utf8_lossy(stderr);
    let mut lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() > MAX_LINES {
        lines = lines.split_off(lines.len().saturating_sub(MAX_LINES));
    }
    let mut out = lines.join("\n");
    if out.chars().count() > MAX_CHARS {
        out = tail_chars(&out, MAX_CHARS);
    }
    if out.trim().is_empty() {
        "<no stderr output>".to_string()
    } else {
        out
    }
}

fn tail_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_string();
    }
    let skip = total - max_chars;
    let start = s
        .char_indices()
        .nth(skip)
        .map(|(i, _)| i)
        .unwrap_or(0);
    s[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_ffmpeg_stderr_reader_returns_fallback_on_panic() {
        let join_err: Box<dyn std::any::Any + Send> = Box::new("boom");
        let got = join_ffmpeg_stderr_reader(Err(join_err));
        assert!(!got.is_empty());
        assert!(String::from_utf8_lossy(&got).contains("panicked"));
    }

    #[test]
    fn ogg_fallback_attempted_when_default_decode_fails() {
        let mut default_called = 0;
        let mut fallback_called = 0;

        let path = Path::new("track.ogg");
        let res = decode_ogg_with_fallback::<()>(
            path,
            || {
                default_called += 1;
                Err("default failed".to_string())
            },
            || {
                fallback_called += 1;
                Err("fallback failed".to_string())
            },
        );

        assert!(res.is_err());
        assert_eq!(default_called, 1);
        assert_eq!(fallback_called, 1);
    }

    #[test]
    fn ogg_fallback_not_attempted_when_default_decode_succeeds() {
        let mut default_called = 0;
        let mut fallback_called = 0;

        let path = Path::new("track.ogg");
        let res = decode_ogg_with_fallback::<i32>(
            path,
            || {
                default_called += 1;
                Ok(123)
            },
            || {
                fallback_called += 1;
                Ok(456)
            },
        );

        assert_eq!(res.unwrap(), 123);
        assert_eq!(default_called, 1);
        assert_eq!(fallback_called, 0);
    }

    #[test]
    fn find_ffmpeg_from_reports_not_found_and_lists_searched_paths_in_order() {
        // Use a temp dir as "exe_dir" with no ffmpeg present, and a PATH with two temp dirs.
        let exe_dir = tempfile::tempdir().unwrap();
        let path_a = tempfile::tempdir().unwrap();
        let path_b = tempfile::tempdir().unwrap();

        let path_env = std::env::join_paths([path_a.path(), path_b.path()]).unwrap();
        let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };

        let err = find_ffmpeg_from(exe_name, Some(exe_dir.path()), Some(path_env.as_os_str()))
            .unwrap_err();

        assert!(err.to_lowercase().contains("ffmpeg not found"));
        assert!(err.contains("Searched locations:"));

        // Order assertions (high-signal): we should see near-binary checks first, then PATH lookup,
        // then PATH dir entries in the same order as provided.
        let idx_near = err.find("near binary:").unwrap();
        let idx_near_tools = err.find("near binary tools:").unwrap();
        let idx_path_lookup = err.find("PATH lookup").unwrap();
        let idx_path_dir_a = err
            .find(&format!("PATH dir: `{}`", path_a.path().join(exe_name).display()))
            .unwrap();
        let idx_path_dir_b = err
            .find(&format!("PATH dir: `{}`", path_b.path().join(exe_name).display()))
            .unwrap();

        assert!(idx_near < idx_near_tools);
        assert!(idx_near_tools < idx_path_lookup);
        assert!(idx_path_lookup < idx_path_dir_a);
        assert!(idx_path_dir_a < idx_path_dir_b);
    }

    #[test]
    fn find_ffmpeg_from_prefers_near_binary_over_path() {
        let exe_dir = tempfile::tempdir().unwrap();
        let path_dir = tempfile::tempdir().unwrap();

        let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };

        // Create ffmpeg next to "binary".
        let near = exe_dir.path().join(exe_name);
        std::fs::write(&near, b"").unwrap();

        // Also create one on PATH (should not be chosen).
        let on_path = path_dir.path().join(exe_name);
        std::fs::write(&on_path, b"").unwrap();
        let path_env = std::env::join_paths([path_dir.path()]).unwrap();

        let (found, searched) =
            find_ffmpeg_from(exe_name, Some(exe_dir.path()), Some(path_env.as_os_str())).unwrap();

        assert_eq!(found, near);
        assert!(searched.iter().any(|s| s.contains("near binary:")));
        assert!(!searched.iter().any(|s| s.contains("PATH lookup")), "PATH should not be consulted once found near binary");
    }

    #[test]
    fn trim_ffmpeg_stderr_handles_unicode_and_does_not_panic() {
        let mut stderr = Vec::new();
        for i in 0..200 {
            stderr.extend_from_slice(format!("line {i} 😀 αβγ\n").as_bytes());
        }
        stderr.extend_from_slice(b"TAIL_MARK\n");

        let trimmed = trim_ffmpeg_stderr(&stderr);
        assert!(trimmed.contains("TAIL_MARK"));
        assert!(trimmed.chars().count() <= 2000);
    }

    #[test]
    fn trim_ffmpeg_stderr_handles_invalid_utf8_and_large_content() {
        let mut stderr = Vec::new();
        // Invalid UTF-8 sequence followed by lots of data to force trimming.
        stderr.extend_from_slice(&[0xF0, 0x9F]); // truncated 4-byte sequence
        for _ in 0..5000 {
            stderr.extend_from_slice(b"x");
        }
        stderr.extend_from_slice(b"\nTAIL_MARK_2\n");

        let trimmed = trim_ffmpeg_stderr(&stderr);
        assert!(trimmed.contains("TAIL_MARK_2"));
        assert!(trimmed.chars().count() <= 2000);
    }

    #[test]
    fn read_to_end_capped_keeps_only_last_n_bytes() {
        use std::io::Cursor;

        let cap = 64;
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(b"PREFIX_SHOULD_BE_DROPPED_");
        input.extend_from_slice(&vec![b'a'; 500]);
        input.extend_from_slice(b"TAIL_MARK_3");

        let expected = input[input.len() - cap..].to_vec();
        let mut cursor = Cursor::new(input);
        let got = read_to_end_capped(&mut cursor, cap);

        assert_eq!(got.len(), cap);
        assert_eq!(got, expected);
        assert!(String::from_utf8_lossy(&got).contains("TAIL_MARK_3"));
    }
}
