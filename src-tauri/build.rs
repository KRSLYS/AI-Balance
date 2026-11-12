fn main() {
    // The edition is chosen when the app is built: see `Edition` in src/model.rs.
    println!("cargo:rerun-if-env-changed=AIBALANCE_EDITION");
    tauri_build::build()
}
