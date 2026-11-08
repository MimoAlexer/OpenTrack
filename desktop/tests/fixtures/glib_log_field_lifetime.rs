use glib::{gstr, LogField};

pub fn retain_expired_bytes() -> LogField<'static> {
    let value = vec![b'x'];
    LogField::new(gstr!("MESSAGE"), &value)
}

pub fn release_bytes_before_reading_field() {
    let value = vec![b'x'];
    let field = LogField::new(gstr!("MESSAGE"), &value);
    drop(value);
    let _ = field.value_bytes();
}
