use glib::{prelude::*, subclass::prelude::*};
use std::{path::PathBuf, process::Command, sync::OnceLock};

mod imp {
    use glib::{prelude::*, subclass::prelude::*};
    use std::sync::{Mutex, OnceLock};

    #[derive(Default)]
    pub struct BindingObject {
        pub text: Mutex<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BindingObject {
        const NAME: &'static str = "OpenTrackBindingBoundaryControl";
        type Type = super::BindingObject;
    }

    impl ObjectImpl for BindingObject {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: OnceLock<Vec<glib::ParamSpec>> = OnceLock::new();
            PROPERTIES.get_or_init(|| vec![glib::ParamSpecString::builder("text").build()])
        }

        fn property(&self, _id: usize, _pspec: &glib::ParamSpec) -> glib::Value {
            self.text.lock().unwrap().to_value()
        }

        fn set_property(&self, _id: usize, value: &glib::Value, _pspec: &glib::ParamSpec) {
            *self.text.lock().unwrap() = value.get().unwrap();
        }
    }
}

glib::wrapper! {
    pub struct BindingObject(ObjectSubclass<imp::BindingObject>);
}

#[test]
fn owned_and_invocation_borrowed_property_transforms_remain_bidirectional() {
    let source: BindingObject = glib::Object::new();
    let target: BindingObject = glib::Object::new();
    let binding = source
        .bind_property("text", &target, "text")
        .bidirectional()
        .transform_to(|binding, value: String| {
            assert!(binding.source().is_some());
            Some(format!("to:{value}"))
        })
        .transform_from(|binding, value: String| {
            assert!(binding.target().is_some());
            Some(format!("from:{value}"))
        })
        .build();
    source.set_property("text", "one");
    assert_eq!(target.property::<String>("text"), "to:one");
    target.set_property("text", "two");
    assert_eq!(source.property::<String>("text"), "from:two");
    binding.unbind();

    let _binding = source
        .bind_property("text", &target, "text")
        .bidirectional()
        .transform_to_with_values(|binding, value| {
            assert!(binding.source().is_some());
            Some(format!("borrow-to:{}", value.get::<&str>().unwrap()).to_value())
        })
        .transform_from_with_values(|binding, value| {
            assert!(binding.target().is_some());
            Some(format!("borrow-from:{}", value.get::<&str>().unwrap()).to_value())
        })
        .build();
    source.set_property("text", "three");
    assert_eq!(target.property::<String>("text"), "borrow-to:three");
    target.set_property("text", "four");
    assert_eq!(source.property::<String>("text"), "borrow-from:four");
}

#[test]
fn thread_safe_implementation_references_keep_strong_and_weak_transport() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<glib::subclass::ObjectImplRef<imp::BindingObject>>();
    assert_send_sync::<glib::subclass::ObjectImplWeakRef<imp::BindingObject>>();

    let object: BindingObject = glib::Object::new();
    object.set_property("text", "owned");
    let strong = object.imp().ref_counted();
    let weak = strong.downgrade();
    std::thread::spawn(move || {
        let upgraded = weak.upgrade().unwrap();
        assert_eq!(upgraded.obj().property::<String>("text"), "owned");
        assert_eq!(strong.obj().property::<String>("text"), "owned");
    })
    .join()
    .unwrap();
}

struct CompileEnvironment {
    dependencies: PathBuf,
    glib: PathBuf,
}

fn compile(source: &str) -> std::process::Output {
    static ENVIRONMENT: OnceLock<CompileEnvironment> = OnceLock::new();
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let environment = ENVIRONMENT.get_or_init(|| {
        let dependencies = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        let glib = std::fs::read_dir(&dependencies)
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with("libglib-") && name.ends_with(".rlib")
            })
            .max_by_key(|entry| entry.metadata().unwrap().modified().unwrap())
            .expect("the integration test depends on a built GLib library")
            .path();
        CompileEnvironment { dependencies, glib }
    });
    let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "opentrack-glib-object-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let source_path = directory.join("boundary.rs");
    std::fs::write(&source_path, source).unwrap();
    let result = Command::new("rustc")
        .args(["--edition=2024", "--crate-type=lib", "--emit=metadata"])
        .arg(&source_path)
        .arg("--out-dir")
        .arg(&directory)
        .arg("--extern")
        .arg(format!("glib={}", environment.glib.display()))
        .arg("-L")
        .arg(format!("dependency={}", environment.dependencies.display()))
        .output()
        .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    result
}

fn assert_rejected(source: &str, diagnostic: &str) {
    let result = compile(source);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success(), "unsafe program was accepted");
    assert!(
        stderr.contains(diagnostic),
        "unexpected compiler failure: {stderr}"
    );
}

#[test]
fn typed_transforms_reject_escaping_value_and_binding_borrows_in_both_directions() {
    let control = compile(
        "use glib::prelude::*; fn control(a: &glib::Object, b: &glib::Object) { let _ = a.bind_property(\"name\", b, \"name\").transform_to(|_, value: String| Some(value)).transform_from(|_, value: String| Some(value)); }",
    );
    assert!(
        control.status.success(),
        "{}",
        String::from_utf8_lossy(&control.stderr)
    );
    for direction in ["transform_to", "transform_from"] {
        assert_rejected(
            &format!(
                "use glib::prelude::*; static SAVED: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None); fn rejected(a: &glib::Object, b: &glib::Object) {{ let _ = a.bind_property(\"name\", b, \"name\").{direction}(|_, value: &'static str| {{ *SAVED.lock().unwrap() = Some(value); Some(value.to_owned()) }}); }}"
            ),
            "FromValue",
        );
        for borrowed_type in ["Option<&'static str>", "&'static glib::Value"] {
            assert_rejected(
                &format!(
                    "use glib::prelude::*; fn rejected(a: &glib::Object, b: &glib::Object) {{ let _ = a.bind_property(\"name\", b, \"name\").{direction}(|_, _value: {borrowed_type}| Some(String::new())); }}"
                ),
                "FromValue",
            );
        }
        assert_rejected(
            &format!(
                "use glib::prelude::*; thread_local! {{ static SAVED: std::cell::RefCell<Option<&'static glib::Binding>> = const {{ std::cell::RefCell::new(None) }}; }} fn rejected(a: &glib::Object, b: &glib::Object) {{ let _ = a.bind_property(\"name\", b, \"name\").{direction}(|binding, value: String| {{ SAVED.with(|saved| *saved.borrow_mut() = Some(binding)); Some(value) }}); }}"
            ),
            "borrowed data escapes",
        );
    }
}

#[test]
fn implementation_references_obey_non_thread_safe_parent() {
    let hierarchy = r#"
use glib::{prelude::*, subclass::prelude::*};
mod parent {
    use glib::subclass::prelude::*;
    #[derive(Default)] pub struct Parent { pub value: std::cell::RefCell<String> }
    #[glib::object_subclass] impl ObjectSubclass for Parent {
        const NAME: &'static str = "OpenTrackBoundaryParent";
        type Type = super::Parent;
    }
    impl ObjectImpl for Parent {}
}
glib::wrapper! { pub struct Parent(ObjectSubclass<parent::Parent>); }
pub trait ParentImpl: ObjectImpl {}
unsafe impl<T: ParentImpl> IsSubclassable<T> for Parent {}
mod child {
    use glib::subclass::prelude::*;
    #[derive(Default)] pub struct Child;
    #[glib::object_subclass] impl ObjectSubclass for Child {
        const NAME: &'static str = "OpenTrackBoundaryChild";
        type Type = super::Child;
        type ParentType = super::Parent;
    }
    impl ObjectImpl for Child {}
    impl super::ParentImpl for Child {}
}
glib::wrapper! { pub struct Child(ObjectSubclass<child::Child>) @extends Parent; }
fn check() { CHECK }
"#;
    let control = compile(&hierarchy.replace(
        "CHECK",
        "fn safe<T: Send + Sync>() {} safe::<child::Child>();",
    ));
    assert!(
        control.status.success(),
        "{}",
        String::from_utf8_lossy(&control.stderr)
    );
    let inherited_control = compile(
        &hierarchy
            .replace("std::cell::RefCell", "std::sync::Mutex")
            .replace(
                "CHECK",
                "fn safe<T: Send + Sync>() {} safe::<glib::subclass::ObjectImplRef<child::Child>>(); safe::<glib::subclass::ObjectImplWeakRef<child::Child>>();",
            ),
    );
    assert!(
        inherited_control.status.success(),
        "{}",
        String::from_utf8_lossy(&inherited_control.stderr)
    );
    for wrapper in ["ObjectImplRef", "ObjectImplWeakRef"] {
        for thread_trait in ["Send", "Sync"] {
            assert_rejected(
                &hierarchy.replace("CHECK", &format!("fn requires<T: {thread_trait}>() {{}} requires::<glib::subclass::{wrapper}<child::Child>>();")),
                "cannot be shared between threads safely",
            );
        }
    }
}
