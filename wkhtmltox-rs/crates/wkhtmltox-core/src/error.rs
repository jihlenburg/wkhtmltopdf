// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WkError {
    #[error("load failed: {url} (http {http_status:?})")]
    Load { url: String, http_status: Option<u16> },
    #[error("render error: {0}")]
    Render(String),
    #[error("pagination error: {0}")]
    Pagination(String),
    #[error("pdf error: {0}")]
    Pdf(String),
    #[error("xslt error: {0}")]
    Xslt(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("bad argument: {0}")]
    BadArg(String),
    #[error("engine error: {0}")]
    Engine(String),
    #[error("security policy: {0}")]
    Security(String),
}

pub type Result<T> = std::result::Result<T, WkError>;
