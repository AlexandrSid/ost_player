use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error(
        "portable app folder is not writable (data dir: {data_dir}). \
Try moving the app folder to a writable location (e.g. your Desktop/Documents, or another non-system folder). \
Underlying error: {source}"
    )]
    PortableNotWritable {
        data_dir: PathBuf,
        source: std::io::Error,
    },

    #[error("config error: {message}")]
    Config { message: String },

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;
