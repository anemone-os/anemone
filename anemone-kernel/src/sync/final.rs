/// A simple wrapper for once-initialized data.
///
/// Nothing more than a `Option<T>`, but with more explicit semantics.
///
/// Note the difference between [Final] and [core::cell::OnceCell]. The latter
/// can be re-initialized multiple times, which is not what we want.
#[derive(Debug, Clone, Copy)]
pub struct Final<T> {
    value: Option<T>,
}

impl<T> Final<T> {
    pub const fn new_uninit() -> Self {
        Self { value: None }
    }

    pub const fn new_init(value: T) -> Self {
        Self { value: Some(value) }
    }

    pub fn init(&mut self, value: T) {
        assert!(self.value.is_none(), "Final: already initialized");
        self.value = Some(value);
    }

    /// # Panics
    ///
    /// Panics if the value is not initialized.
    pub fn get(&self) -> &T {
        self.value.as_ref().expect("Final: not initialized")
    }

    /// # Panics
    ///
    /// Panics if the value is not initialized.
    pub fn get_mut(&mut self) -> &mut T {
        self.value.as_mut().expect("Final: not initialized")
    }
}

impl<T> AsRef<T> for Final<T> {
    fn as_ref(&self) -> &T {
        self.get()
    }
}

impl<T> AsMut<T> for Final<T> {
    fn as_mut(&mut self) -> &mut T {
        self.get_mut()
    }
}
