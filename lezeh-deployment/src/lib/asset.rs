use rust_embed::RustEmbed;

// Path relative to Cargo.toml dir
#[derive(RustEmbed)]
#[folder = "./assets/templates/"]
pub struct Asset;
