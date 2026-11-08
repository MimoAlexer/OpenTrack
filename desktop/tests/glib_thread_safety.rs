use glib::{
    BoxedAnyObject, SendValue, SendWeakRef, Type, Value, prelude::*, thread_guard::ThreadGuard,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

struct LocalDrop {
    drops: Arc<AtomicUsize>,
    _local: Rc<()>,
}

impl Drop for LocalDrop {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

fn local_drop(drops: &Arc<AtomicUsize>) -> LocalDrop {
    LocalDrop {
        drops: drops.clone(),
        _local: Rc::new(()),
    }
}

#[test]
fn owner_drops_and_extracts_thread_guard_exactly_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    drop(ThreadGuard::new(local_drop(&drops)));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let value = ThreadGuard::new(local_drop(&drops)).into_inner();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    drop(value);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}

#[test]
fn foreign_thread_guard_drop_does_not_destroy_local_value_during_unwind() {
    let drops = Arc::new(AtomicUsize::new(0));
    let guard = ThreadGuard::new(local_drop(&drops));
    assert!(
        thread::spawn(move || catch_unwind(AssertUnwindSafe(|| drop(guard))).is_err())
            .join()
            .unwrap()
    );
    assert_eq!(drops.load(Ordering::SeqCst), 0);
}

#[test]
fn foreign_thread_guard_extraction_does_not_destroy_local_value_during_unwind() {
    let drops = Arc::new(AtomicUsize::new(0));
    let guard = ThreadGuard::new(local_drop(&drops));
    assert!(
        thread::spawn(move || {
            catch_unwind(AssertUnwindSafe(|| drop(guard.into_inner()))).is_err()
        })
        .join()
        .unwrap()
    );
    assert_eq!(drops.load(Ordering::SeqCst), 0);
}

#[test]
fn empty_send_weak_references_are_bound_before_they_can_be_populated() {
    for weak in [
        SendWeakRef::<BoxedAnyObject>::new(),
        SendWeakRef::<BoxedAnyObject>::default(),
    ] {
        let object = BoxedAnyObject::new(Rc::new(()));
        weak.set(Some(&object));
        assert!(weak.upgrade().is_some());
        assert!(
            thread::spawn(move || catch_unwind(AssertUnwindSafe(|| {
                let _ = std::ops::Deref::deref(&weak);
            }))
            .is_err())
            .join()
            .unwrap()
        );
    }
}

#[test]
fn foreign_empty_send_weak_reference_cannot_escape_into_a_weak_reference() {
    let weak = SendWeakRef::<BoxedAnyObject>::new();
    assert!(
        thread::spawn(move || catch_unwind(AssertUnwindSafe(|| {
            drop(weak.into_weak_ref());
        }))
        .is_err())
        .join()
        .unwrap()
    );
}

#[test]
fn send_weak_clone_checks_owner_before_acquiring_a_strong_reference() {
    let object = BoxedAnyObject::new(Rc::new(()));
    let weak = SendWeakRef::from(object.downgrade());
    let owner_clone = weak.clone();
    assert_eq!(owner_clone.upgrade().as_ref(), Some(&object));
    assert!(
        thread::spawn(move || catch_unwind(AssertUnwindSafe(|| drop(weak.clone()))).is_err())
            .join()
            .unwrap()
    );
    assert_eq!(owner_clone.upgrade().as_ref(), Some(&object));
}

#[test]
fn send_weak_reference_can_still_be_dropped_on_another_thread() {
    let object = BoxedAnyObject::new(Rc::new(()));
    let weak = SendWeakRef::from(object.downgrade());
    thread::spawn(move || drop(weak)).join().unwrap();
    assert_eq!(*object.borrow::<Rc<()>>(), Rc::new(()));
}

thread_local! {
    static LOCAL_OBJECT: BoxedAnyObject = BoxedAnyObject::new(Rc::new(()));
}

struct MisleadingConverter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "OpenTrackSendValueTestEnum")]
enum TestEnum {
    First,
    Second,
}

#[derive(Clone, glib::Boxed)]
#[boxed_type(name = "OpenTrackLocalSendValueTestBoxed")]
struct LocalBoxed(Rc<()>);

impl StaticType for MisleadingConverter {
    fn static_type() -> Type {
        BoxedAnyObject::static_type()
    }
}

impl ToValue for MisleadingConverter {
    fn to_value(&self) -> Value {
        LOCAL_OBJECT.with(ToValue::to_value)
    }

    fn value_type(&self) -> Type {
        Self::static_type()
    }
}

impl From<MisleadingConverter> for Value {
    fn from(_: MisleadingConverter) -> Self {
        LOCAL_OBJECT.with(ToValue::to_value)
    }
}

#[test]
fn send_converter_does_not_certify_its_erased_non_send_payload() {
    assert!(catch_unwind(|| SendValue::from_owned(MisleadingConverter)).is_err());
    assert!(catch_unwind(|| MisleadingConverter.to_send_value()).is_err());
    LOCAL_OBJECT.with(|object| {
        assert!(
            object
                .to_value()
                .try_into_send_value::<MisleadingConverter>()
                .is_err()
        );
        // Rejection leaves the owner and its original state available locally.
        assert_eq!(Rc::strong_count(&object.borrow::<Rc<()>>()), 1);
    });
}

#[test]
fn nested_values_cannot_hide_local_objects_during_send_promotion() {
    struct Nested;
    impl From<Nested> for Value {
        fn from(_: Nested) -> Self {
            LOCAL_OBJECT.with(|object| glib::value::BoxedValue(object.to_value()).to_value())
        }
    }
    assert!(catch_unwind(|| SendValue::from_owned(Nested)).is_err());
}

#[test]
fn arbitrary_boxed_payloads_are_not_certified_by_a_send_converter() {
    struct BoxedConverter;
    impl From<BoxedConverter> for Value {
        fn from(_: BoxedConverter) -> Self {
            LocalBoxed(Rc::new(())).to_value()
        }
    }
    assert!(catch_unwind(|| SendValue::from_owned(BoxedConverter)).is_err());
    let local = LocalBoxed(Rc::new(()));
    assert_eq!(Rc::strong_count(&local.0), 1);
}

#[test]
fn native_scalar_and_string_send_values_still_cross_threads() {
    let values = [
        42i32.to_send_value(),
        SendValue::from_owned(String::from("example")),
        Some(String::from("optional")).to_send_value(),
        Type::STRING.to_send_value(),
    ];
    thread::spawn(move || {
        assert_eq!(values[0].get::<i32>().unwrap(), 42);
        assert_eq!(values[1].get::<String>().unwrap(), "example");
        assert_eq!(
            values[2].get::<Option<String>>().unwrap().as_deref(),
            Some("optional")
        );
        assert_eq!(values[3].get::<Type>().unwrap(), Type::STRING);
    })
    .join()
    .unwrap();
}

#[test]
fn native_boxed_enum_and_flags_send_values_still_cross_threads() {
    let mut checksum = glib::Checksum::new(glib::ChecksumType::Sha256).unwrap();
    checksum.update(b"example");
    let expected_checksum = checksum.clone().string().unwrap();
    let values = [
        glib::Bytes::from_owned(vec![1u8, 2, 3]).to_send_value(),
        glib::Date::from_dmy(8, glib::DateMonth::November, 2026)
            .unwrap()
            .to_send_value(),
        glib::DateTime::from_unix_utc(1234).unwrap().to_send_value(),
        glib::TimeZone::utc().to_send_value(),
        glib::Error::new(glib::FileError::Failed, "example").to_send_value(),
        glib::VariantType::new("s").unwrap().to_send_value(),
        vec![String::from("first"), String::from("second")].to_send_value(),
        42i32.to_variant().to_send_value(),
        TestEnum::Second.to_send_value(),
        glib::IOCondition::IN.to_send_value(),
        checksum.to_send_value(),
    ];
    thread::spawn(move || {
        assert_eq!(values[0].get::<glib::Bytes>().unwrap().as_ref(), &[1, 2, 3]);
        assert_eq!(values[1].get::<glib::Date>().unwrap().year(), 2026);
        assert_eq!(values[2].get::<glib::DateTime>().unwrap().to_unix(), 1234);
        assert_eq!(values[3].get::<glib::TimeZone>().unwrap().offset(0), 0);
        assert_eq!(values[4].get::<glib::Error>().unwrap().message(), "example");
        assert_eq!(values[5].get::<glib::VariantType>().unwrap().as_str(), "s");
        assert_eq!(values[6].get::<Vec<String>>().unwrap(), ["first", "second"]);
        assert_eq!(
            values[7].get::<glib::Variant>().unwrap().get::<i32>(),
            Some(42)
        );
        assert_eq!(values[8].get::<TestEnum>().unwrap(), TestEnum::Second);
        assert_eq!(
            values[9].get::<glib::IOCondition>().unwrap(),
            glib::IOCondition::IN
        );
        assert_eq!(
            values[10]
                .get::<glib::Checksum>()
                .unwrap()
                .string()
                .unwrap(),
            expected_checksum
        );
    })
    .join()
    .unwrap();
}

#[test]
fn local_result_handles_remain_local_and_send_results_remain_transferable() {
    // Inference is ambiguous if a supposedly local handle also implements
    // Send. This checks both wrappers without running an unsafe cross-thread
    // non-Send result transfer.
    trait AmbiguousIfSend<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    let _ = <glib::JoinHandle<Rc<()>> as AmbiguousIfSend<_>>::marker;
    let _ = <glib::SpawnWithinJoinHandle<Rc<()>> as AmbiguousIfSend<_>>::marker;
    fn require_send<T: Send>() {}
    require_send::<glib::JoinHandle<i32>>();
    require_send::<glib::SpawnWithinJoinHandle<i32>>();

    let context = glib::MainContext::new();
    let handle = context.spawn_local(async { Rc::new(42) });
    assert_eq!(*context.block_on(handle).unwrap(), 42);
    let handle = context.spawn_from_within(|| {
        let local = Rc::new(43);
        async move { *local }
    });
    assert_eq!(context.block_on(handle).unwrap(), 43);
}

#[test]
fn completed_send_result_handle_can_be_received_on_another_thread() {
    let context = glib::MainContext::new();
    let handle = context.spawn_local(async { 44i32 });
    context.iteration(false);
    let result = thread::spawn(move || glib::MainContext::new().block_on(handle).unwrap())
        .join()
        .unwrap();
    assert_eq!(result, 44);
}

#[test]
fn discarding_a_completed_local_result_drops_it_on_its_owner() {
    let drops = Arc::new(AtomicUsize::new(0));
    let value = local_drop(&drops);
    let context = glib::MainContext::new();
    let handle = context.spawn_local(async move { value });
    context.iteration(false);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(handle);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn foreign_spawn_request_can_still_construct_a_local_future_on_its_context() {
    let context = glib::MainContext::new();
    let _owner = context.acquire().unwrap();
    let destination = context.clone();
    let handle = thread::spawn(move || {
        destination.spawn_from_within(|| {
            let local = Rc::new(45i32);
            async move { *local }
        })
    })
    .join()
    .unwrap();
    assert_eq!(context.block_on(handle).unwrap(), 45);
}

#[test]
fn native_context_loop_and_parameter_spec_values_remain_transferable() {
    let context = glib::MainContext::new();
    let main_loop = glib::MainLoop::new(Some(&context), false);
    let parameter = glib::ParamSpecString::builder("example-property")
        .default_value("example")
        .build();
    let typed_parameter = parameter
        .clone()
        .downcast::<glib::ParamSpecString>()
        .unwrap();
    let values = [
        context.to_send_value(),
        SendValue::from_owned(main_loop),
        parameter.to_send_value(),
        typed_parameter
            .to_value()
            .try_into_send_value::<glib::ParamSpecString>()
            .unwrap(),
    ];
    thread::spawn(move || {
        let context = values[0].get::<glib::MainContext>().unwrap();
        context.wakeup();
        let main_loop = values[1].get::<glib::MainLoop>().unwrap();
        assert_eq!(main_loop.context(), context);
        assert!(!main_loop.is_running());
        assert_eq!(
            values[2].get::<glib::ParamSpec>().unwrap().name(),
            "example-property"
        );
        assert_eq!(
            values[3]
                .get::<glib::ParamSpecString>()
                .unwrap()
                .default_value(),
            Some("example")
        );
    })
    .join()
    .unwrap();
}

#[test]
fn variant_backing_owners_are_released_before_all_send_value_promotions() {
    struct LocalSerialized {
        bytes: [u8; 1],
        _local: LocalDrop,
    }
    impl AsRef<[u8]> for LocalSerialized {
        fn as_ref(&self) -> &[u8] {
            &self.bytes
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let variant = glib::Variant::from_data::<u8, _>(LocalSerialized {
        bytes: [7],
        _local: local_drop(&drops),
    });
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let bytes = variant.data_as_bytes();
    let values = [
        variant.to_send_value(),
        SendValue::from_owned(variant.clone()),
        variant
            .to_value()
            .try_into_send_value::<glib::Variant>()
            .unwrap(),
        bytes.to_send_value(),
    ];
    drop(variant);
    drop(bytes);
    thread::spawn(move || {
        for value in &values[..3] {
            assert_eq!(value.get::<glib::Variant>().unwrap().get::<u8>(), Some(7));
        }
        assert_eq!(values[3].get::<glib::Bytes>().unwrap().as_ref(), &[7]);
    })
    .join()
    .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn native_sources_closures_and_string_builders_remain_transferable() {
    let source = glib::idle_source_new(None, glib::Priority::DEFAULT, || glib::ControlFlow::Break);
    let closure =
        glib::Closure::new(|arguments| Some((arguments[0].get::<i32>().unwrap() + 1).to_value()));
    let values = [
        source.to_send_value(),
        SendValue::from_owned(closure),
        glib::GStringBuilder::new("example")
            .to_value()
            .try_into_send_value::<glib::GStringBuilder>()
            .unwrap(),
    ];
    thread::spawn(move || {
        let source = values[0].get::<glib::Source>().unwrap();
        assert!(!source.is_destroyed());
        source.destroy();
        let closure = values[1].get::<glib::Closure>().unwrap();
        // This is the Rust closure above with a known argument/return signature.
        let result = unsafe { closure.invoke_with_values(Type::I32, &[1i32.to_value()]) }.unwrap();
        assert_eq!(result.get::<i32>().unwrap(), 2);
        assert_eq!(
            values[2].get::<glib::GStringBuilder>().unwrap().as_str(),
            "example"
        );
    })
    .join()
    .unwrap();
}
