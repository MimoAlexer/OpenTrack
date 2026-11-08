use std::{io::Write, path::PathBuf, process};

fn linked_glib() -> (PathBuf, PathBuf) {
    let executable = std::env::current_exe().unwrap();
    let deps = executable.parent().unwrap().to_owned();
    let stem = executable.file_stem().unwrap().to_str().unwrap();
    let (target, hash) = stem.rsplit_once('-').unwrap();
    let fingerprints = deps.parent().unwrap().join(".fingerprint");
    let fingerprint = fingerprints
        .join(format!("{}-{hash}", env!("CARGO_PKG_NAME")))
        .join(format!("test-integration-test-{target}.json"));
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fingerprint).unwrap()).unwrap();
    let expected = metadata["deps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|dependency| dependency[1] == "glib")
        .unwrap()[3]
        .as_u64()
        .unwrap()
        .to_le_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    // The pinned Cargo toolchain records the exact dependency fingerprint in
    // this test binary's metadata. Other feature modes may coexist in deps.
    let libraries: Vec<_> = std::fs::read_dir(fingerprints)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("glib-") {
                return None;
            }
            let fingerprint = std::fs::read_to_string(entry.path().join("lib-glib")).ok()?;
            let library = deps.join(format!("lib{name}.rlib"));
            (fingerprint.trim() == expected && library.is_file()).then_some(library)
        })
        .collect();
    assert_eq!(
        libraries.len(),
        1,
        "cannot resolve the linked GLib artifact"
    );
    (libraries[0].clone(), deps)
}

pub fn compile_fixture(crate_name: &str, source: &[u8]) -> process::Output {
    let (library, deps) = linked_glib();
    let output_path =
        std::env::temp_dir().join(format!("opentrack-{crate_name}-{}.rmeta", process::id()));
    let mut compiler =
        process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .args([
                "--edition=2021",
                "--crate-type=lib",
                "--emit=metadata",
                "--crate-name",
                crate_name,
                "--extern",
            ])
            .arg(format!("glib={}", library.display()))
            .arg("-L")
            .arg(format!("dependency={}", deps.display()))
            .arg("-o")
            .arg(&output_path)
            .arg("-")
            .stdin(process::Stdio::piped())
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .spawn()
            .unwrap();
    compiler.stdin.take().unwrap().write_all(source).unwrap();
    let output = compiler.wait_with_output().unwrap();
    let _ = std::fs::remove_file(output_path);
    output
}
