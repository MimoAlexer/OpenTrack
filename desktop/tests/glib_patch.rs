use glib::prelude::*;

#[test]
fn optimized_variant_string_iterator_uses_writable_output_pointer() {
    let value = ["alpha", "beta", "gamma", "delta"].to_variant();
    assert_eq!(value.array_iter_str().unwrap().next(), Some("alpha"));
    assert_eq!(value.array_iter_str().unwrap().last(), Some("delta"));
    assert_eq!(value.array_iter_str().unwrap().nth(2), Some("gamma"));
    assert_eq!(value.array_iter_str().unwrap().next_back(), Some("delta"));
    assert_eq!(value.array_iter_str().unwrap().nth_back(2), Some("beta"));
    let mut mixed = value.array_iter_str().unwrap();
    assert_eq!(mixed.next(), Some("alpha"));
    assert_eq!(mixed.next_back(), Some("delta"));
    assert_eq!(mixed.collect::<Vec<_>>(), ["beta", "gamma"]);
}
