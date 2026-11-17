//! The only crate Swift sees. Thin UniFFI wrappers over `cinma-core`;
//! no business logic lives here.

use std::path::PathBuf;
use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CinmaError {
    #[error("{message}")]
    Database { message: String },
    #[error("{message}")]
    Io { message: String },
    #[error("{message}")]
    Internal { message: String },
}

impl From<cinma_core::CoreError> for CinmaError {
    fn from(error: cinma_core::CoreError) -> Self {
        use cinma_core::CoreError as E;
        let message = error.to_string();
        match error {
            E::Database(_) => Self::Database { message },
            E::Io(_) => Self::Io { message },
            E::Poisoned => Self::Internal { message },
        }
    }
}

/// Version of the Rust core.
#[uniffi::export]
pub fn core_version() -> String {
    cinma_core::VERSION.to_string()
}

#[derive(uniffi::Object)]
pub struct CinmaCore {
    inner: cinma_core::Core,
}

#[uniffi::export]
impl CinmaCore {
    /// Opens the core. Blocking (touches disk) — call off the main thread.
    #[uniffi::constructor]
    pub fn open(data_dir: String) -> Result<Arc<Self>, CinmaError> {
        let inner = cinma_core::Core::open(&PathBuf::from(data_dir))?;
        Ok(Arc::new(Self { inner }))
    }

    pub fn schema_version(&self) -> Result<u32, CinmaError> {
        Ok(self.inner.schema_version()?)
    }
}
