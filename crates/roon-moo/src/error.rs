#[derive(Debug, thiserror::Error)]
pub enum MooError {
    #[error("empty message")]
    Empty,
    #[error("missing protocol prefix: expected \"MOO/1\", got {0:?}")]
    InvalidProtocol(String),
    #[error("missing verb in first line")]
    MissingVerb,
    #[error("unknown verb: {0:?}")]
    UnknownVerb(String),
    #[error("missing name in first line")]
    MissingName,
    #[error("missing required header: Request-Id")]
    MissingRequestId,
    #[error("invalid Request-Id: {0:?}")]
    InvalidRequestId(String),
    #[error("Content-Length present but Content-Type missing")]
    ContentLengthWithoutContentType,
    #[error("invalid Content-Length: {0:?}")]
    InvalidContentLength(String),
    #[error("body length {actual} does not match Content-Length {expected}")]
    BodyLengthMismatch { expected: usize, actual: usize },
    #[error("invalid UTF-8 in headers")]
    InvalidHeaderUtf8,
    #[error("malformed header line: {0:?}")]
    MalformedHeader(String),
    #[error("invalid JSON body: {0}")]
    InvalidJson(#[from] serde_json::Error),
}
