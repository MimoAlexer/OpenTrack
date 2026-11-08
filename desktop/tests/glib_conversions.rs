use glib::{
    Date, DateMonth, GString, GStringBuilder, NormalizeMode, Value, prelude::*, translate::*,
};
use std::{cell::Cell, ffi::CString};

struct ChangingView<T> {
    first: T,
    later: T,
    calls: Cell<usize>,
}

impl<T> ChangingView<T> {
    fn new(first: T, later: T) -> Self {
        Self {
            first,
            later,
            calls: Cell::new(0),
        }
    }
}

impl AsRef<str> for ChangingView<&str> {
    fn as_ref(&self) -> &str {
        let call = self.calls.get();
        self.calls.set(call + 1);
        if call == 0 { self.first } else { self.later }
    }
}

impl AsRef<[char]> for ChangingView<&[char]> {
    fn as_ref(&self) -> &[char] {
        let call = self.calls.get();
        self.calls.set(call + 1);
        if call == 0 { self.first } else { self.later }
    }
}

#[test]
fn utf8_conversion_uses_one_input_view() {
    for later in ["", "a longer second view"] {
        let input = ChangingView::new("A🤔ț", later);
        assert_eq!(glib::utf8_to_utf32(&input).as_slice(), ['A', '🤔', 'ț']);
        assert_eq!(input.calls.get(), 1);
    }
}

#[test]
fn utf32_conversion_uses_one_input_view() {
    let first = ['A', '🤔', 'ț'];
    let longer = ['x'; 32];
    for later in [&[][..], &longer[..]] {
        let input = ChangingView::new(first.as_slice(), later);
        assert_eq!(glib::utf32_to_utf8(&input), "A🤔ț");
        assert_eq!(input.calls.get(), 1);
    }
}

#[test]
fn casefold_uses_one_input_view() {
    for later in ["", "a longer second view"] {
        let input = ChangingView::new("Straße", later);
        assert_eq!(glib::casefold(&input), "strasse");
        assert_eq!(input.calls.get(), 1);
    }
}

#[test]
fn normalize_uses_one_input_view() {
    for later in ["", "a longer second view"] {
        let input = ChangingView::new("é", later);
        assert_eq!(glib::normalize(&input, NormalizeMode::Default), "e\u{301}");
        assert_eq!(input.calls.get(), 1);
    }
}

#[test]
fn foreign_string_bytes_keep_the_complete_payload() {
    let source = "a foreign glib allocation with a unicode suffix 🤔";
    assert_eq!(glib::casefold(source).into_bytes(), source.as_bytes());
}

#[test]
fn foreign_string_byte_ownership_preserves_empty_and_nul_contracts() {
    for source in [
        "",
        "x",
        "🤔",
        "a string long enough to require foreign storage",
    ] {
        let expected_with_nul = [source.as_bytes(), &[0]].concat();
        assert_eq!(glib::casefold(source).into_bytes(), source.as_bytes());
        assert_eq!(
            glib::casefold(source).into_bytes_with_nul(),
            expected_with_nul
        );
        assert_eq!(Vec::<u8>::from(glib::casefold(source)), expected_with_nul);
        let cstring = CString::try_from(glib::casefold(source)).unwrap();
        assert_eq!(cstring.as_bytes_with_nul(), expected_with_nul);
    }
}

#[test]
fn native_and_inline_string_byte_ownership_stays_compatible() {
    for source in [
        "",
        "x",
        "🤔",
        "a string longer than the inline representation",
    ] {
        let expected_with_nul = [source.as_bytes(), &[0]].concat();
        assert_eq!(GString::from(source).into_bytes(), source.as_bytes());
        assert_eq!(
            GString::from(source).into_bytes_with_nul(),
            expected_with_nul
        );
        let native = GString::try_from(CString::new(source).unwrap()).unwrap();
        assert_eq!(native.into_bytes(), source.as_bytes());
    }
}

#[test]
fn copied_string_builder_keeps_its_payload_length_and_nul() {
    for source in ["", "x", "🤔", "a longer builder payload"] {
        let copied = GStringBuilder::new(source).clone().into_string();
        assert_eq!(copied.as_str(), source);
        assert_eq!(
            copied.into_bytes_with_nul(),
            [source.as_bytes(), &[0]].concat()
        );
        let cstring = CString::try_from(GStringBuilder::new(source).clone().into_string()).unwrap();
        assert_eq!(cstring.as_bytes(), source.as_bytes());
    }
}

#[test]
fn boxed_inline_builder_copies_release_all_owned_buffers() {
    let builders: Vec<_> = (0..16).map(|_| GStringBuilder::new("")).collect();
    let full: *mut glib::ffi::GString = builders.as_slice().to_glib_full();
    let copied: Vec<GStringBuilder> =
        unsafe { FromGlibContainer::from_glib_full_num(full, builders.len()) };
    for builder in copied {
        assert_eq!(builder.into_string().as_str(), "");
    }
}

#[test]
fn owned_string_vector_values_own_every_string() {
    for strings in [
        vec![],
        vec![GString::from("")],
        vec![GString::from("one"), GString::from("🤔")],
        vec![
            GString::from("short"),
            GString::from("a string longer than the inline representation"),
            GString::try_from(CString::new("native storage").unwrap()).unwrap(),
        ],
    ] {
        let expected = strings.clone();
        let value = Value::from(strings);
        let copy = value.clone();
        drop(value);
        assert_eq!(copy.get::<Vec<GString>>().unwrap(), expected);
        assert_eq!(expected.to_value().get::<Vec<GString>>().unwrap(), expected);
    }
}

#[test]
fn consumed_string_vector_preserves_c_string_interior_nul_semantics() {
    let string = GString::from_string_unchecked("before\0after".into());
    let value = Value::from(vec![string]);
    assert_eq!(
        value.get::<Vec<GString>>().unwrap(),
        [GString::from("before")]
    );
}

#[test]
fn boxed_inline_full_and_container_slices_copy_every_element() {
    for length in [0, 1, 2, 256] {
        let dates: Vec<_> = (0..length)
            .map(|day| Date::from_dmy((day % 28 + 1) as u8, DateMonth::January, 2026).unwrap())
            .collect();
        let full: *mut glib::ffi::GDate = dates.as_slice().to_glib_full();
        let copied: Vec<Date> = unsafe { FromGlibContainer::from_glib_full_num(full, length) };
        assert_eq!(copied, dates);
        let (container, _storage) =
            <Date as ToGlibContainerFromSlice<'_, *mut glib::ffi::GDate>>::to_glib_container_from_slice(
                &dates,
            );
        let copied: Vec<Date> = unsafe { FromGlibContainer::from_glib_full_num(container, length) };
        assert_eq!(copied, dates);

        let values: Vec<_> = (0..length)
            .map(|i| format!("value {i}").to_value())
            .collect();
        let full: *mut glib::gobject_ffi::GValue = values.as_slice().to_glib_full();
        let copied: Vec<Value> = unsafe { FromGlibContainer::from_glib_full_num(full, length) };
        drop(values);
        for (i, value) in copied.iter().enumerate() {
            assert_eq!(value.get::<String>().unwrap(), format!("value {i}"));
        }
    }
}
