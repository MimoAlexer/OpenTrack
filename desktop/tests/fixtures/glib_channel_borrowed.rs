#![allow(deprecated)]

pub fn attach_borrowed(text: &str) -> glib::SourceId {
    let (sender, receiver) = glib::MainContext::channel(glib::Priority::DEFAULT);
    sender.send(text).unwrap();
    drop(sender);
    receiver.attach(None, |_: &str| glib::ControlFlow::Continue)
}
