use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),
    #[error("Required dependency is missing: {0}")]
    DependencyMissing(String),
    #[error("The download engine could not be started: {0}")]
    Process(String),
    /// yt-dlp ran but could not read the link; the message is already explained.
    #[error("{0}")]
    Analysis(String),
    #[error("The download engine returned data this app could not read: {0}")]
    Parse(String),
    #[error("Tool installation failed: {0}")]
    Install(String),
    #[error("Local data could not be accessed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("That download could not be found")]
    NotFound,
}

pub type AppResult<T> = Result<T, AppError>;
