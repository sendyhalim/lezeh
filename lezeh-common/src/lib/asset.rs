use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../assets/templates/"]
pub struct Asset;
