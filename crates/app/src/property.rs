//! Setting a QObject's read-only properties from Rust.

/// Sets a read-only property, notifying QML only when the value changes.
macro_rules! set {
    ($self:ident, $field:ident, $changed:ident, $value:expr) => {{
        let value = $value;
        if $self.$field != value {
            $self.as_mut().rust_mut().$field = value;
            $self.as_mut().$changed();
        }
    }};
}

pub(crate) use set;
