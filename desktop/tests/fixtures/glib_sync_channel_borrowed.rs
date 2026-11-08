#![allow(deprecated)]

pub fn attach_borrowed(text: &str) -> glib::SourceId {
    let (sender, receiver) = glib::MainContext::sync_channel(glib::Priority::DEFAULT, 1);
    sender.try_send(text).unwrap();
    drop(sender);
    receiver.attach(None, |_: &str| glib::ControlFlow::Continue)
}
