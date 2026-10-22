fn main() {
    // SQLx embeds migrations in the binary. Watch the directory so newly added
    // migration files invalidate the build, not only changes to existing files.
    println!("cargo:rerun-if-changed=migrations");
}
