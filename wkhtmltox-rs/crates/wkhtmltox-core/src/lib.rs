// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
#![forbid(unsafe_code)]
pub mod assembly;
pub mod compat;
pub mod error;
pub mod headerfooter;
pub mod outline;
pub mod pdfread;
pub mod registry;
pub mod render;
pub mod settings;
pub mod testing;
pub mod toc;
pub use error::{Result, WkError};
