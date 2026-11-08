use glib::{Error, FileError, List, PtrSlice, SList, Slice, StrV, translate::ToGlibPtr};

fn error(message: &str) -> Error {
    Error::new(FileError::Failed, message)
}

macro_rules! linked_list_checks {
    ($module:ident, $list:ident, $native:ty, $length:path, $copy:path, $free:path) => {
        mod $module {
            use super::*;

            #[test]
            fn retain_removes_head_then_later_nodes() {
                let mut list: $list<Error> = ["first", "second", "third"]
                    .into_iter()
                    .map(error)
                    .collect();
                let mut visits = 0;
                list.retain(|_| {
                    visits += 1;
                    false
                });
                assert_eq!(visits, 3);
                assert!(list.is_empty());
                list.push_front(error("after-empty"));
                assert_eq!(list.front().unwrap().message(), "after-empty");
            }

            #[test]
            fn retain_predicate_observes_real_values() {
                let mut list: $list<Error> = ["remove", "keep-one", "remove", "keep-two"]
                    .into_iter()
                    .map(error)
                    .collect();
                list.retain(|value| value.message().starts_with("keep"));
                assert_eq!(
                    list.iter().map(|value| value.message()).collect::<Vec<_>>(),
                    ["keep-one", "keep-two"]
                );
                list.retain(|value| value.message().starts_with("keep"));
                assert_eq!(list.len(), 2);
            }

            #[test]
            fn retain_empty_list_never_calls_predicate() {
                let mut list = $list::<Error>::new();
                list.retain(|_| panic!("empty list has no values"));
                assert!(list.is_empty());
            }

            #[test]
            fn container_transfer_copies_nodes_and_borrows_values() {
                for size in [0, 1, 4] {
                    let list: $list<Error> = (0..size).map(|_| error("retained")).collect();
                    let raw: *mut $native = list.to_glib_container().0;
                    unsafe {
                        assert_eq!($length(raw) as usize, size);
                        let mut source = list.as_ptr();
                        let mut copied = raw;
                        while !source.is_null() {
                            assert_ne!(source, copied);
                            assert_eq!((*source).data, (*copied).data);
                            source = (*source).next;
                            copied = (*copied).next;
                        }
                        assert!(copied.is_null());
                        $free(raw);
                    }
                    assert_eq!(list.len(), size);
                    assert!(list.iter().all(|value| value.message() == "retained"));
                }
            }

            #[test]
            fn incoming_container_owns_clones_and_preserves_source_values() {
                let list: $list<Error> = ["first", "second"].into_iter().map(error).collect();
                let raw: *mut $native = unsafe { $copy(list.as_ptr() as *mut _) };
                let cloned = unsafe { $list::<Error>::from_glib_container(raw) };
                assert_eq!(
                    cloned
                        .iter()
                        .map(|value| value.message())
                        .collect::<Vec<_>>(),
                    ["first", "second"]
                );
                drop(list);
                assert_eq!(cloned.front().unwrap().message(), "first");
                drop(cloned);
                let empty = unsafe { $list::<Error>::from_glib_container(std::ptr::null_mut()) };
                assert!(empty.is_empty());
            }
        }
    };
}

linked_list_checks!(
    doubly_linked,
    List,
    glib::ffi::GList,
    glib::ffi::g_list_length,
    glib::ffi::g_list_copy,
    glib::ffi::g_list_free
);
linked_list_checks!(
    singly_linked,
    SList,
    glib::ffi::GSList,
    glib::ffi::g_slist_length,
    glib::ffi::g_slist_copy,
    glib::ffi::g_slist_free
);

#[test]
fn strv_clear_can_drop_or_transfer_empty_storage() {
    for transfer in [false, true] {
        let mut values = StrV::from(["one", "two", "three"]);
        values.clear();
        assert!(values.is_empty());
        values.clear();
        if transfer {
            assert!(values.into_raw().is_null());
        } else {
            drop(values);
        }
    }
    let mut unallocated = StrV::new();
    unallocated.clear();
}

#[test]
fn strv_empty_allocations_support_drop_clone_and_reuse() {
    for capacity in [0, 1, 32] {
        let mut values = StrV::with_capacity(capacity);
        assert!(values.is_empty());
        drop(values.clone());
        values.reserve(64);
        values.push("after-reserve".into());
        values.clear();
        values.push("after-clear".into());
        assert_eq!(values[0].as_str(), "after-clear");
        values.clear();
        drop(values);
        drop(StrV::with_capacity(capacity));
    }
}

#[test]
fn strv_partial_extension_remains_terminated_if_as_ref_panics() {
    struct Input(bool);
    impl AsRef<str> for Input {
        fn as_ref(&self) -> &str {
            assert!(!self.0, "synthetic conversion failure");
            "first"
        }
    }

    let mut values = StrV::from(["old-first", "old-second", "old-third"]);
    values.clear();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        values.extend_from_slice(&[Input(false), Input(true)]);
    }));
    assert!(result.is_err());
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].as_str(), "first");
    unsafe {
        assert!((*values.as_ptr().add(values.len())).is_null());
    }
    values.push("after-panic".into());
    assert_eq!(values[1].as_str(), "after-panic");
}

#[test]
fn pointer_slice_partial_extension_remains_terminated_if_clone_panics() {
    #[repr(transparent)]
    struct Item(Error);

    impl Clone for Item {
        fn clone(&self) -> Self {
            assert_ne!(self.0.message(), "panic", "synthetic clone failure");
            Self(self.0.clone())
        }
    }

    impl glib::translate::GlibPtrDefault for Item {
        type GlibType = *mut glib::ffi::GError;
    }

    // Error is a transparent pointer wrapper; Item preserves its representation.
    unsafe impl glib::translate::TransparentPtrType for Item {}

    let mut values = PtrSlice::<Item>::new();
    values.push(Item(error("old-first")));
    values.push(Item(error("old-second")));
    values.push(Item(error("old-third")));
    values.clear();
    let input = [Item(error("first")), Item(error("panic"))];
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        values.extend_from_slice(&input);
    }));
    assert!(result.is_err());
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].0.message(), "first");
    unsafe {
        assert!((*values.as_ptr().add(values.len())).is_null());
    }
    values.push(Item(error("after-panic")));
    assert_eq!(values[1].0.message(), "after-panic");
}

#[test]
fn empty_slice_to_ptr_slice_has_valid_storage_for_push() {
    for reserve in [false, true] {
        let mut source = Slice::<Error>::new();
        if reserve {
            source.reserve(8);
        }
        let mut converted = PtrSlice::from(source);
        assert!(converted.is_empty());
        assert_eq!(converted.capacity(), 0);
        converted.push(error("after-conversion"));
        assert_eq!(converted[0].message(), "after-conversion");
        unsafe {
            assert!((*converted.as_ptr().add(converted.len())).is_null());
        }
    }
}

#[test]
fn empty_ptr_slice_to_slice_has_valid_storage_for_push() {
    for reserve in [false, true] {
        let mut source = PtrSlice::<Error>::new();
        if reserve {
            source.reserve(8);
        }
        let mut converted = Slice::from(source);
        assert!(converted.is_empty());
        assert_eq!(converted.capacity(), 0);
        converted.push(error("after-conversion"));
        assert_eq!(converted[0].message(), "after-conversion");
    }
}

#[test]
fn populated_slice_round_trip_preserves_values_and_ownership() {
    let mut values = Slice::<Error>::new();
    values.push(error("one"));
    values.push(error("two"));
    let pointers = PtrSlice::from(values);
    unsafe {
        assert!((*pointers.as_ptr().add(pointers.len())).is_null());
    }
    let mut values = Slice::from(pointers);
    assert_eq!(
        values
            .iter()
            .map(|value| value.message())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    values.clear();
    let mut pointers = PtrSlice::from(values);
    pointers.push(error("after-clear"));
    pointers.clear();
    let mut values = Slice::from(pointers);
    values.push(error("after-round-trip"));
    assert_eq!(values[0].message(), "after-round-trip");
}
