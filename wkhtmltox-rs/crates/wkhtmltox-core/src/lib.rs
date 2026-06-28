// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
#![forbid(unsafe_code)]
pub mod assembly;
pub mod compat;
pub mod error;
pub mod outline;
pub mod pdfread;
pub mod render;
pub mod testing;
pub use error::{Result, WkError};
