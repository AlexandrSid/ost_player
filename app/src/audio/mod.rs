use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub fn try_default_output() -> Result<(rodio::OutputStream, rodio::OutputStreamHandle), String> {
    rodio::OutputStream::try_default()
        .map_err(|_| "audio output unavailable (no default output device?)".to_string())
}

pub fn decode_file(path: &Path) -> Result<rodio::Decoder<BufReader<File>>, String> {
    let f = File::open(path).map_err(|e| format!("failed to open `{}`: {e}", path.display()))?;
    let reader = BufReader::new(f);
    rodio::Decoder::new(reader).map_err(|e| format!("failed to decode `{}`: {e}", path.display()))
}

