use opentrack_desktop::credentials::{CredentialRef, SystemVault, Vault};
use uuid::Uuid;
fn main() {
    let reference = CredentialRef {
        user_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
    };
    let origin = "https://opentrack-keyring-probe.invalid";
    let vault = SystemVault;
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    println!(
        "Temporary probe reference: {} {}",
        reference.user_id, reference.session_id
    );
    let result = (|| {
        vault.set(origin, &reference, &token)?;
        let loaded = vault.get(origin, &reference)?;
        if loaded.as_deref().map(|s| s.as_str()) != Some(token.as_str()) {
            return Err(opentrack_desktop::protocol::NativeError::storage());
        }
        vault.delete(origin, &reference)?;
        if vault.get(origin, &reference)?.is_some() {
            return Err(opentrack_desktop::protocol::NativeError::storage());
        }
        Ok(())
    })();
    let _ = vault.delete(origin, &reference);
    match result {
        Ok(()) => println!("Secret Service write/read/delete confirmed."),
        Err(error) => {
            eprintln!("Secret Service probe failed: {}", error.code);
            std::process::exit(1);
        }
    }
}
