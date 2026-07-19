//! Generic spec loader — thin wrapper around `tatara_lisp::compile_typed`.
//!
//! Mirrors sui-spec's `loader.rs`. Parses a Lisp source string into a
//! typed `TataraDomain` value. Call sites use this so they don't import
//! `tatara_lisp` directly, and so the `(defk8sview)`/`(defk8saction)`
//! authored specs can be driven by both any future engine + tests.

use tatara_lisp::TataraDomain;

use crate::error::SpecError;

/// Compile the entire `src` and return every top-level form of type `T`.
///
/// # Errors
///
/// Returns a `SpecError::Compile` if the source fails to read,
/// macroexpand, or compile under `T`'s schema.
pub fn load_all<T: TataraDomain>(src: &str) -> Result<Vec<T>, SpecError> {
    Ok(tatara_lisp::compile_typed::<T>(src)?)
}

impl From<tatara_lisp::LispError> for SpecError {
    fn from(err: tatara_lisp::LispError) -> Self {
        SpecError::Compile(err.to_string())
    }
}
