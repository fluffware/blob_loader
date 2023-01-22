use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
pub struct BlobInfo {
    pub start: u32,
    pub size: u32,
    pub checksum: [u8; 20],
    pub filename: String,
}

#[derive(Serialize, Deserialize)]
pub struct ProbeInfo {
    pub chip: String,
}

#[derive(Serialize,Deserialize)]
pub struct BlobInfoFile {
    pub info: HashMap<String, BlobInfo>,
    pub probe: ProbeInfo,
}

