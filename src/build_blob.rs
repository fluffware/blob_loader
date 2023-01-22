use crate::blob_info::{BlobInfo, BlobInfoFile, ProbeInfo};
use crate::link_script_parser;
use serde_derive::Deserialize;
use sha1_smol::Sha1;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use toml;

#[derive(Deserialize)]
struct BlobParams {
    filename: String,
    inline: Option<bool>, // Blob is part of the executable. Overrides inline-dev and inline-release
    inline_dev: Option<bool>, // Blob is part of the executable for dev profiles
    inline_release: Option<bool>, // Blob is part of the executable for release profiles
}

#[derive(Deserialize)]
struct BlobConfig {
    files: HashMap<String, BlobParams>,
    probe: ProbeInfo,
}

#[derive(Debug)]
struct Blob {
    name: String,
    start: u32,
    size: u32,
    checksum: [u8; 20],
    filename: String,
    inline: bool,
}

type DynResult<T> = Result<T, Box<dyn std::error::Error>>;
const BLOB_FILE: &str = "Blobs.toml";
fn read_blobs(release: bool) -> DynResult<(Vec<Blob>, ProbeInfo)> {
    let top_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let blob_file = top_dir.join(BLOB_FILE);
    let mut total_size = 0;
    let mut file = File::open(&blob_file)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).unwrap();
    let blob_config: BlobConfig = toml::from_str(&buf).unwrap();
    let mut blobs = Vec::new();
    for (name, params) in blob_config.files {
        let mut cs = Sha1::new();
        let mut buf = [0u8; 1024];
        let mut file_size = 0;
        let filename = top_dir.join(&params.filename);
        let mut f = File::open(&params.filename)?;
        loop {
            let r = f.read(&mut buf)?;
            if r == 0 {
                break;
            }
            cs.update(&buf[..r]);
            file_size += r;
        }
        let blob = Blob {
            name,
            start: total_size,
            size: u32::try_from(file_size)?,
            checksum: cs.digest().bytes(),
            filename: filename
                .as_path()
                .to_str()
                .ok_or_else(|| "Filename can not be converted to UTF-8")?
                .to_string(),
            inline: params.inline.unwrap_or_else(|| {
                if release {
                    params.inline_release.unwrap_or(true)
                } else {
                    params.inline_dev.unwrap_or(false)
                }
            }),
        };
        if !blob.inline {
            // Only loaded blobs need space
            total_size += u32::try_from(file_size)?;
        }
        blobs.push(blob);
    }
    Ok((blobs, blob_config.probe))
}

fn build_source<F>(out_file: &mut F, blobs: &[Blob], origin: u32) -> DynResult<()>
where
    F: Write,
{
    out_file.write(
        r#"
use core::slice;
use sha1_smol::Sha1;
"#
        .as_bytes(),
    )?;
    for blob in blobs {
        if blob.inline {
            out_file.write(
                format!(
                    r#"
pub fn {0}() ->  &'static [u8] {{
include_bytes!("{1}")
}}"#,
                    blob.name, blob.filename,
                )
                .as_bytes(),
            )?;
        } else {
            out_file.write(
                format!(
                    r#"
pub fn {0}() ->  &'static [u8] {{
    let blob = unsafe{{slice::from_raw_parts(0x{1:x} as *const u8, {2})}}
;
    let mut m = Sha1::new();
    let checksum:[u8;20] = [{3}];
    m.update(blob);
    if &m.digest().bytes() != &checksum {{
        panic!("Checksum check failed for {0}");
    }}
    blob
}}"#,
                    blob.name,
                    blob.start + origin,
                    blob.size,
                    blob.checksum.map(|v| v.to_string()).join(","),
                )
                .as_bytes(),
            )?;
        }
    }
    Ok(())
}

fn build_link_script<I, O>(in_file: &mut I, out_file: &mut O, length: i64) -> DynResult<i64>
where
    I: Read,
    O: Write,
{
    let mut in_buf = String::new();
    in_file.read_to_string(&mut in_buf)?;
    let (after, (before, (name, attr, origin, flash_length))) =
        link_script_parser::find_memory_def(&in_buf, "FLASH")
            .map_err(|e| format!("Failed to parse link script: {}", e))?;
    let mut out_buf = before.to_string();
    out_buf += &format!(
        "{} {}: ORIGIN = 0x{:x}, LENGTH = 0x{:x}",
        name,
        if let Some(attr) = attr {
            format!("({})", attr)
        } else {
            "".to_string()
        },
        origin,
        flash_length - length
    );
    out_buf += after;
    out_file.write_all(out_buf.as_bytes())?;
    Ok(origin + flash_length - length)
}

fn env_dir(var_name: &str) -> DynResult<PathBuf> {
    Ok(PathBuf::from(env::var(var_name).map_err(|_| {
        format!("Environment variable '{}' not found", var_name)
    })?))
}

fn env_str(var_name: &str) -> DynResult<String> {
    Ok(env::var(var_name).map_err(|_| format!("Environment variable '{}' not found", var_name))?)
}

fn build_blob_info<O>(out_file: &mut O, blobs: &[Blob], origin: u32, chip: &str) -> DynResult<()>
where
    O: Write,
{
    let mut info = HashMap::<String, BlobInfo>::new();
    for blob in blobs {
        if !blob.inline {
            info.insert(
                blob.name.to_string(),
                BlobInfo {
                    size: blob.size,
                    checksum: blob.checksum,
                    start: blob.start + origin,
                    filename: blob.filename.clone(),
                },
            );
        }
    }
    let buf = toml::to_vec(&BlobInfoFile {
        info,
        probe: ProbeInfo {
            chip: chip.to_string(),
        },
    })?;
    out_file.write(&buf)?;
    Ok(())
}

pub fn prepare_blob() -> DynResult<()> {
    let top_dir = env_dir("CARGO_MANIFEST_DIR")?;
    let out_dir = env_dir("OUT_DIR")?;
    let target_dir = env_dir("CARGO_TARGET_DIR").unwrap_or_else(|_| top_dir.join("target"));
    let profile = env_str("PROFILE")?;
    let (blobs, probe) = read_blobs(profile == "release")?;
    let last_blob = blobs.last().ok_or_else(|| "No blobs defined")?;
    let total_size = last_blob.start + last_blob.size;
    let mut link_out = File::create(out_dir.join("memory.x"))?;
    let mut link_in = File::open(top_dir.join("memory.x"))?;

    let flash_end = build_link_script(&mut link_in, &mut link_out, i64::from(total_size))?;
    // Tell the compiler where to find memory.x
    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed={}", BLOB_FILE);

    let mut info_file = File::create(target_dir.join("BlobInfo.toml"))?;
    let blob_start = u32::try_from(flash_end)?;
    build_blob_info(&mut info_file, &blobs, blob_start, &probe.chip)?;

    let mut source = File::create(out_dir.join("blob.rs"))?;
    build_source(&mut source, &blobs, blob_start)?;
    Ok(())
}
