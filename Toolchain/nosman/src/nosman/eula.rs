use colored::Colorize;
use crate::nosman;
use crate::nosman::common::get_hostname;

pub fn silently_agree_eulas() {
    let engines_dir = nosman::path::get_default_engines_dir(&nosman::workspace::current_root().unwrap());
    if !engines_dir.exists() {
        println!("{}", "No installed Nodos engine found in workspace.".red());
        return;
    }

    // Find EULA_UNCONFIRMED files in each engine and confirm them
    for entry in std::fs::read_dir(engines_dir).expect("Unable to read Engine directory") {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let eula_unconfirmed = path.join("EULA_UNCONFIRMED.json");
        if !eula_unconfirmed.exists() {
            println!("Nodos Engine EULA at {}: {}", path.display(), "Accepted".green());
            continue;
        }
        let eula_str = std::fs::read_to_string(&eula_unconfirmed).expect("Failed to read EULA_UNCONFIRMED file");
        let mut eula_json: serde_json::Value = serde_json::from_str(&eula_str).expect("Failed to parse EULA_UNCONFIRMED file");
        eula_json["accepted"] = serde_json::Value::Bool(true);
        eula_json["acceptation_date_iso"] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());
        let hostname = get_hostname();
        println!("Nodos Engine EULA at {}: {}", path.display(), format!("Accepting ({})", hostname).yellow());

        eula_json["accepting_host_name"] = serde_json::Value::String(hostname);
        // Write back to the file
        std::fs::write(&eula_unconfirmed, eula_json.to_string()).expect("Failed to write EULA_UNCONFIRMED file");
        // Move the file to EULA_CONFIRMED.json
        let eula_confirmed = path.join("EULA_CONFIRMED.json");
        std::fs::rename(&eula_unconfirmed, &eula_confirmed).expect("Failed to rename EULA_UNCONFIRMED file");
    }
    println!("{}", "All Nodos EULAs are accepted.".green());
}