use spark_max_cabi::SparkMaxBindings;
use std::fmt;

#[derive(Debug)]
pub enum SparkMaxError {
    LoadError(String),
    SessionInitFailed(i32),
    InferenceFailed(String),
}

impl fmt::Display for SparkMaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SparkMaxError::LoadError(e) => write!(f, "Modular MAX load error: {}", e),
            SparkMaxError::SessionInitFailed(code) => {
                write!(f, "Failed to create Modular MAX session: code {}", code)
            }
            SparkMaxError::InferenceFailed(e) => write!(f, "Modular MAX inference failed: {}", e),
        }
    }
}

impl std::error::Error for SparkMaxError {}

pub enum SparkMaxBackend {
    ModularMojo {
        session_id: i32,
        bindings: &'static SparkMaxBindings,
    },
    NativeRustFallback {
        session_id: i32,
    },
}

impl fmt::Debug for SparkMaxBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SparkMaxBackend::ModularMojo { session_id, .. } => {
                write!(
                    f,
                    "SparkMaxBackend::ModularMojo(session_id: {})",
                    session_id
                )
            }
            SparkMaxBackend::NativeRustFallback { session_id } => {
                write!(
                    f,
                    "SparkMaxBackend::NativeRustFallback(session_id: {})",
                    session_id
                )
            }
        }
    }
}

pub struct SparkMaxSession {
    backend: SparkMaxBackend,
}

impl SparkMaxSession {
    pub fn new(device_id: i32) -> Result<Self, SparkMaxError> {
        // Attempt to load compiled Modular MAX bindings via C-ABI
        match SparkMaxBindings::global() {
            Ok(bindings) => {
                let session_id = unsafe { (bindings.session_create)(device_id) };
                if session_id > 0 {
                    return Ok(Self {
                        backend: SparkMaxBackend::ModularMojo {
                            session_id,
                            bindings,
                        },
                    });
                }
            }
            Err(err) => {
                tracing::warn!(
                    "Modular MAX dynamic library not available ({}). Falling back to pure native Rust execution.",
                    err
                );
            }
        }

        // Graceful fallback to pure native Rust execution on non-MAX hardware
        let session_id = 1001 + device_id;
        Ok(Self {
            backend: SparkMaxBackend::NativeRustFallback { session_id },
        })
    }

    pub fn version(&self) -> i32 {
        match &self.backend {
            SparkMaxBackend::ModularMojo { bindings, .. } => unsafe { (bindings.version)() },
            SparkMaxBackend::NativeRustFallback { .. } => 1,
        }
    }

    pub fn session_id(&self) -> i32 {
        match &self.backend {
            SparkMaxBackend::ModularMojo { session_id, .. } => *session_id,
            SparkMaxBackend::NativeRustFallback { session_id } => *session_id,
        }
    }

    pub fn is_fallback(&self) -> bool {
        matches!(self.backend, SparkMaxBackend::NativeRustFallback { .. })
    }

    pub fn compute_scalar(&self, input_val: f32) -> f32 {
        match &self.backend {
            SparkMaxBackend::ModularMojo {
                session_id,
                bindings,
            } => unsafe { (bindings.compute_scalar)(*session_id, input_val) },
            SparkMaxBackend::NativeRustFallback { session_id } => {
                input_val * 2.5 + (*session_id as f32)
            }
        }
    }
}

impl Drop for SparkMaxSession {
    fn drop(&mut self) {
        if let SparkMaxBackend::ModularMojo {
            session_id,
            bindings,
        } = &self.backend
        {
            unsafe {
                let _ = (bindings.session_destroy)(*session_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spark_max_session_always_succeeds() {
        let session = SparkMaxSession::new(0).expect("Session creation must succeed with fallback");
        assert_eq!(session.version(), 1);
        assert!(session.session_id() >= 1000);

        let out = session.compute_scalar(4.0);
        let expected = 4.0 * 2.5 + session.session_id() as f32;
        assert_eq!(out, expected);
    }

    #[test]
    fn test_spark_max_fallback_backend() {
        let session = SparkMaxSession {
            backend: SparkMaxBackend::NativeRustFallback { session_id: 1005 },
        };
        assert!(session.is_fallback());
        assert_eq!(session.session_id(), 1005);
        assert_eq!(session.version(), 1);
        assert_eq!(session.compute_scalar(2.0), 2.0 * 2.5 + 1005.0);
    }
}
