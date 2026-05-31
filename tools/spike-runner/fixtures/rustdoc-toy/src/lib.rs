#![feature(staged_api)]
#![stable(feature = "rustdoc_toy", since = "1.0.0")]

//! Tiny rustdoc JSON fixture for the anneal code-as-corpus spike.

/// Stable buffer type consumers can rely on.
#[stable(feature = "rustdoc_toy_buffer", since = "1.0.0")]
pub struct Buffer {
    value: usize,
}

impl Buffer {
    /// Build a stable buffer.
    #[stable(feature = "rustdoc_toy_buffer_new", since = "1.0.0")]
    pub fn new() -> Self {
        Self { value: 0 }
    }

    /// Experimental push path.
    ///
    /// ```
    /// let mut buffer = rustdoc_toy::Buffer::new();
    /// buffer.push_unstable(1);
    /// ```
    #[unstable(feature = "rustdoc_toy_push", issue = "1")]
    pub fn push_unstable(&mut self, value: usize) {
        self.value = value;
    }
}

/// Stable helper used by deprecated callers.
#[stable(feature = "rustdoc_toy_stable_helper", since = "1.0.0")]
pub fn stable_helper() -> Buffer {
    Buffer::new()
}

/// Old helper retained for compatibility.
#[deprecated(since = "1.1.0", note = "use rustdoc_toy::stable_helper")]
#[stable(feature = "rustdoc_toy_old_helper", since = "1.0.0")]
pub fn old_helper() -> Buffer {
    stable_helper()
}

/// Unstable convenience wrapper that depends on [`old_helper`].
#[unstable(feature = "rustdoc_toy_experimental", issue = "2")]
pub fn experimental_pipeline() -> Buffer {
    old_helper()
}
