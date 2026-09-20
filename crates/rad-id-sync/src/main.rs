use rad_id_sync::identity::IdentityAnchor;
use rad_id_sync::keys::sync_identity_keys;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("status");

    match cmd {
        "status" => {
            let anchor = match IdentityAnchor::load_or_init() {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("Error loading identity anchor: {}", e);
                    std::process::exit(1);
                }
            };
            println!("=== Radicle Identity Anchor ===");
            println!("DID:         {}", anchor.did);
            println!("Name:        {}", anchor.name);
            println!("Type:        {:?}", anchor.entity_type);
            println!(
                "Radicle Key: {}",
                anchor.primary_radicle_key.as_deref().unwrap_or("<none>")
            );
            println!(
                "SSH Key:     {}",
                anchor.primary_ssh_key.as_deref().unwrap_or("<none>")
            );
            println!("Last Synced: {}", anchor.last_synced_at);
            println!("Key History: {} records", anchor.key_history.len());
        }
        "sync" => {
            let anchor = IdentityAnchor::load_or_init().expect("Failed to load anchor");
            let (_, report) = sync_identity_keys(anchor).expect("Sync failed");
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
        "json" => {
            let anchor = IdentityAnchor::load_or_init().expect("Failed to load anchor");
            let (_, report) = sync_identity_keys(anchor).expect("Sync failed");
            println!("{}", serde_json::to_string(&report).unwrap());
        }
        _ => {
            println!("Usage: rad-id-sync [status|sync|json]");
        }
    }
}
