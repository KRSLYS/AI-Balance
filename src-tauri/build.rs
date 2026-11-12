fn main() {
    // The edition is chosen when the app is built: see `Edition` in src/model.rs.
    println!("cargo:rerun-if-env-changed=AIBALANCE_EDITION");
    println!("cargo:rerun-if-env-changed=AIBALANCE_AD_UNIT");
    tauri_build::build()
}
