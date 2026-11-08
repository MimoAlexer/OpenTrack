#![allow(deprecated)]

use glib::{ControlFlow, MainContext, Priority};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex},
    thread::{self, ThreadId},
};

#[path = "support/glib_compile.rs"]
mod glib_compile;

struct LocalDrop {
    drops: Arc<Mutex<Vec<ThreadId>>>,
    _local: Rc<()>,
}

impl LocalDrop {
    fn new(drops: &Arc<Mutex<Vec<ThreadId>>>) -> Self {
        Self {
            drops: drops.clone(),
            _local: Rc::new(()),
        }
    }
}

impl Drop for LocalDrop {
    fn drop(&mut self) {
        self.drops.lock().unwrap().push(thread::current().id());
    }
}

#[test]
fn local_non_send_items_dispatch_and_drop_on_owner() {
    let context = MainContext::new();
    let _owner = context.acquire().unwrap();
    let drops = Arc::new(Mutex::new(Vec::new()));
    let received = Rc::new(Cell::new(false));
    let received_callback = received.clone();
    let (sender, receiver) = MainContext::channel(Priority::DEFAULT);
    sender.send(LocalDrop::new(&drops)).unwrap();
    drop(sender);
    receiver.attach(Some(&context), move |_: LocalDrop| {
        received_callback.set(true);
        ControlFlow::Break
    });

    assert!(context.iteration(false));
    assert!(received.get());
    assert_eq!(*drops.lock().unwrap(), [thread::current().id()]);
}

#[test]
fn unattached_channels_preserve_borrowed_payload_support() {
    let text = String::from("borrowed until the channel is dropped");
    let (sender, receiver) = MainContext::channel::<&str>(Priority::DEFAULT);
    sender.send(&text).unwrap();
    drop(sender);
    drop(receiver);

    let (sender, receiver) = MainContext::sync_channel::<&str>(Priority::DEFAULT, 1);
    sender.try_send(&text).unwrap();
    drop(sender);
    drop(receiver);
    assert_eq!(text, "borrowed until the channel is dropped");
}

#[test]
fn owned_send_items_cross_threads_and_reach_local_callbacks() {
    let context = MainContext::new();
    let _owner = context.acquire().unwrap();
    let received = Rc::new(Cell::new(false));
    let received_callback = received.clone();
    let (sender, receiver) = MainContext::sync_channel(Priority::DEFAULT, 1);
    receiver.attach(Some(&context), move |item: String| {
        assert_eq!(item, "owned cross-thread item");
        received_callback.set(true);
        ControlFlow::Break
    });
    thread::spawn(move || {
        sender
            .send(String::from("owned cross-thread item"))
            .unwrap()
    })
    .join()
    .unwrap();
    assert!(context.iteration(false));
    assert!(received.get());
}

#[test]
fn foreign_source_finalization_keeps_local_queue_on_owner() {
    let context = MainContext::new();
    let _owner = context.acquire().unwrap();
    let drops = Arc::new(Mutex::new(Vec::new()));
    let (sender, receiver) = MainContext::channel(Priority::DEFAULT);
    sender.send(LocalDrop::new(&drops)).unwrap();
    drop(sender);
    let id = receiver.attach(Some(&context), |_: LocalDrop| ControlFlow::Continue);
    let source = context.find_source_by_id(&id).unwrap();

    thread::spawn(move || {
        source.destroy();
        drop(source);
    })
    .join()
    .unwrap();

    assert!(drops.lock().unwrap().is_empty());
    while context.pending() {
        context.iteration(false);
    }
    assert_eq!(*drops.lock().unwrap(), [thread::current().id()]);
}

#[test]
fn foreign_finalization_preserves_a_local_sender_until_owner_drop() {
    let context = MainContext::new();
    let _owner = context.acquire().unwrap();
    let drops = Arc::new(Mutex::new(Vec::new()));
    let (sender, receiver) = MainContext::sync_channel(Priority::DEFAULT, 1);
    sender.try_send(LocalDrop::new(&drops)).unwrap();
    let id = receiver.attach(Some(&context), |_: LocalDrop| ControlFlow::Continue);
    let source = context.find_source_by_id(&id).unwrap();
    thread::spawn(move || {
        source.destroy();
        drop(source);
    })
    .join()
    .unwrap();
    while context.pending() {
        context.iteration(false);
    }
    assert!(drops.lock().unwrap().is_empty());
    assert!(sender.try_send(LocalDrop::new(&drops)).is_err());
    drop(sender);
    assert_eq!(
        *drops.lock().unwrap(),
        [thread::current().id(), thread::current().id()]
    );
}

#[test]
fn unowned_context_cannot_drop_local_payloads_on_foreign_finalization() {
    let context = MainContext::new();
    let drops = Arc::new(Mutex::new(Vec::new()));
    let (sender, receiver) = MainContext::channel(Priority::DEFAULT);
    sender.send(LocalDrop::new(&drops)).unwrap();
    drop(sender);
    let callback_owner = LocalDrop::new(&drops);
    let id = receiver.attach(Some(&context), move |_: LocalDrop| {
        let _ = &callback_owner;
        ControlFlow::Continue
    });
    let source = context.find_source_by_id(&id).unwrap();
    thread::spawn(move || {
        source.destroy();
        drop(source);
    })
    .join()
    .unwrap();
    drop(context);
    // With no owner-thread execution available, retaining the payload is safer
    // than running non-Send destructors on this unrelated thread.
    assert!(drops.lock().unwrap().is_empty());
}

#[test]
fn deferred_cleanup_cancellation_cannot_drop_local_payloads_on_foreign_thread() {
    let context = MainContext::new();
    let owner = context.acquire().unwrap();
    let drops = Arc::new(Mutex::new(Vec::new()));
    let (sender, receiver) = MainContext::sync_channel(Priority::DEFAULT, 1);
    sender.try_send(LocalDrop::new(&drops)).unwrap();
    drop(sender);
    let callback_owner = LocalDrop::new(&drops);
    let id = receiver.attach(Some(&context), move |_: LocalDrop| {
        let _ = &callback_owner;
        ControlFlow::Continue
    });
    let source = context.find_source_by_id(&id).unwrap();
    thread::spawn(move || {
        source.destroy();
        drop(source);
    })
    .join()
    .unwrap();
    assert!(context.pending());
    assert!(drops.lock().unwrap().is_empty());
    drop(owner);
    thread::spawn(move || drop(context)).join().unwrap();
    assert!(drops.lock().unwrap().is_empty());
}

#[test]
fn destroyed_source_rejects_a_rendezvous_sender() {
    let context = MainContext::new();
    let _owner = context.acquire().unwrap();
    let (sender, receiver) = MainContext::sync_channel::<u32>(Priority::DEFAULT, 0);
    let id = receiver.attach(Some(&context), |_| ControlFlow::Continue);
    let source = context.find_source_by_id(&id).unwrap();
    let (start, ready) = std::sync::mpsc::channel();
    let (finished, result) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        ready.recv().unwrap();
        finished.send(sender.send(42)).unwrap();
    });
    source.destroy();
    start.send(()).unwrap();
    assert_eq!(
        result
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap(),
        Err(std::sync::mpsc::SendError(42))
    );
    worker.join().unwrap();
    drop(source);
}

#[test]
fn attached_channels_reject_borrowed_payloads_for_both_constructors() {
    for (name, fixture) in [
        (
            "unbounded",
            include_bytes!("fixtures/glib_channel_borrowed.rs").as_slice(),
        ),
        (
            "bounded",
            include_bytes!("fixtures/glib_sync_channel_borrowed.rs").as_slice(),
        ),
    ] {
        let output =
            glib_compile::compile_fixture(&format!("glib_channel_lifetime_{name}"), fixture);
        let errors = String::from_utf8(output.stderr).unwrap();
        assert!(
            !output.status.success(),
            "borrowed {name} channel attachment compiled"
        );
        assert!(
            errors.contains("E0521") && errors.contains("'static"),
            "unexpected compiler failure for {name}: {errors}"
        );
    }
}
