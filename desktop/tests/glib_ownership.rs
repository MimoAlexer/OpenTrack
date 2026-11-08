use glib::{FixedSizeVariantArray, LogField, Variant, gstr, prelude::*};
use std::{cell::Cell, rc::Rc, thread};

#[path = "support/glib_compile.rs"]
mod glib_compile;

struct LocalOwner<T> {
    values: Vec<T>,
    dropped: Rc<Cell<bool>>,
    accesses: Rc<Cell<usize>>,
    thread: thread::ThreadId,
}

impl<T> AsRef<[T]> for LocalOwner<T> {
    fn as_ref(&self) -> &[T] {
        self.accesses.set(self.accesses.get() + 1);
        &self.values
    }
}

impl<T> Drop for LocalOwner<T> {
    fn drop(&mut self) {
        assert_eq!(thread::current().id(), self.thread);
        self.dropped.set(true);
    }
}

fn local_owner<T>(values: Vec<T>) -> (LocalOwner<T>, Rc<Cell<bool>>, Rc<Cell<usize>>) {
    let dropped = Rc::new(Cell::new(false));
    let accesses = Rc::new(Cell::new(0));
    (
        LocalOwner {
            values,
            dropped: dropped.clone(),
            accesses: accesses.clone(),
            thread: thread::current().id(),
        },
        dropped,
        accesses,
    )
}

#[test]
fn log_fields_borrow_live_binary_empty_and_user_data_values() {
    let data = vec![b'a', 0, b'b'];
    let field = LogField::new(gstr!("MESSAGE"), &data);
    assert_eq!(field.key(), "MESSAGE");
    assert_eq!(field.value_bytes(), Some(data.as_slice()));
    assert_eq!(field.value_str(), Some("a\0b"));
    assert_eq!(field.user_data(), None);

    let empty = LogField::new(gstr!("MESSAGE"), &[]);
    assert_eq!(empty.value_bytes(), Some(&[][..]));
    let user_data = LogField::new_user_data(gstr!("LOCAL_DATA"), 42);
    assert_eq!(user_data.value_bytes(), None);
    assert_eq!(user_data.user_data(), Some(42));

    glib::log_structured!(None, glib::LogLevel::Debug, {
        glib::GString::from("DYNAMIC_KEY") => std::ffi::CString::new("temporary").unwrap().to_bytes();
        "MESSAGE" => "formatted {}", 42;
    });
}

#[test]
fn log_field_cannot_outlive_its_value() {
    let output = glib_compile::compile_fixture(
        "glib_lifetime_regression",
        include_bytes!("fixtures/glib_log_field_lifetime.rs"),
    );
    let errors = String::from_utf8(output.stderr).unwrap();
    assert!(!output.status.success(), "expired LogField borrow compiled");
    assert!(
        errors.contains("E0515"),
        "unexpected compiler failure: {errors}"
    );
    assert!(
        errors.contains("E0505"),
        "dropping borrowed storage was not rejected: {errors}"
    );
}

#[test]
fn serialized_constructors_copy_borrowed_storage_before_it_expires() {
    let expected = (String::from("serialized value"), 7u8, 42u32).to_variant();
    for constructor in 0..4 {
        let data = expected.data().to_vec();
        let actual = match constructor {
            0 => Variant::from_data::<(String, u8, u32), _>(data.as_slice()),
            1 => Variant::from_data_with_type(data.as_slice(), expected.type_()),
            // The bytes were serialized by a Variant of exactly this type.
            2 => unsafe { Variant::from_data_trusted::<(String, u8, u32), _>(data.as_slice()) },
            _ => unsafe { Variant::from_data_with_type_trusted(data.as_slice(), expected.type_()) },
        };
        drop(data);
        let retained_bytes = actual.data_as_bytes();
        let control = expected.clone();
        thread::spawn(move || {
            assert_eq!(actual, control);
            drop(actual);
            assert_eq!(&retained_bytes[..], control.data());
        })
        .join()
        .unwrap();
    }
}

#[test]
fn serialized_constructors_release_non_send_owners_on_the_creating_thread() {
    let expected = vec![1u8, 2, 3, 4].to_variant();
    for constructor in 0..4 {
        let (owner, dropped, accesses) = local_owner(expected.data().to_vec());
        let actual = match constructor {
            0 => Variant::from_data::<Vec<u8>, _>(owner),
            1 => Variant::from_data_with_type(owner, expected.type_()),
            // The bytes were serialized by a Variant of exactly this type.
            2 => unsafe { Variant::from_data_trusted::<Vec<u8>, _>(owner) },
            _ => unsafe { Variant::from_data_with_type_trusted(owner, expected.type_()) },
        };
        assert!(dropped.get());
        assert_eq!(accesses.get(), 1);
        let retained_bytes = actual.data_as_bytes();
        let control = expected.clone();
        thread::spawn(move || {
            assert_eq!(actual, control);
            drop(actual);
            assert_eq!(&retained_bytes[..], control.data());
        })
        .join()
        .unwrap();
    }
}

#[test]
fn consuming_fixed_arrays_copy_borrowed_and_non_send_storage() {
    for values in [vec![], vec![42i32], vec![1, 2, 3]] {
        let expected = FixedSizeVariantArray::from(values.clone()).to_variant();
        let borrowed = Variant::from(FixedSizeVariantArray::from(values.as_slice()));
        let (owner, dropped, accesses) = local_owner(values);
        let owned = Variant::from(FixedSizeVariantArray::from(owner));
        assert!(dropped.get());
        assert_eq!(accesses.get(), 1);
        assert_eq!(borrowed.type_().as_str(), "ai");
        thread::spawn(move || {
            assert_eq!(borrowed, expected);
            assert_eq!(owned, expected);
        })
        .join()
        .unwrap();
    }
}

#[test]
fn owned_bytes_can_still_be_shared_without_copying() {
    let expected = vec![1u8, 2, 3].to_variant();
    let data = expected.data().to_vec();
    let pointer = data.as_ptr();
    let bytes = glib::Bytes::from_owned(data);
    assert_eq!(bytes[..].as_ptr(), pointer);
    let actual = Variant::from_bytes::<Vec<u8>>(&bytes);
    drop(bytes);
    thread::spawn(move || assert_eq!(actual, expected))
        .join()
        .unwrap();
}
