use blob_loader::blob_info::BlobInfoFile;
use probe_rs::{flashing::DownloadOptions, Permissions, Session};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

type DynResult<T> = Result<T, Box<dyn std::error::Error>>;

pub fn load_blob(blob_info: &BlobInfoFile) -> DynResult<()> {
    let mut session = Session::auto_attach(&blob_info.probe.chip, Permissions::default())?;
    let mut loader = session.target().flash_loader();
    let mut buf = [0u8; 1024];
    for (name, blob) in &blob_info.info {
	let mut start = blob.start;
	print!("Reading {} at 0x{:x} ...", name,start);
	let mut f = File::open(&blob.filename)?;
	loop {
            let r = f.read(&mut buf)?;
            if r == 0 {
                break;
            }
	    loader.add_data(start as u64, &buf[..r])?;
	    start += r as u32;
        }
	println!("done");

    }
    print!("Flashing ...");
    loader.commit(&mut session, DownloadOptions::default())?;
    println!("done");
    Ok(())
}

pub fn read_blob_info<R>(file: &mut R) -> DynResult<BlobInfoFile>
where
    R: Read,
{
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    let blobs = toml::from_str(&buf)?;
    Ok(blobs)
}

const BLOB_INFO_FILE: &str = "BlobInfo.toml";

fn main() -> ExitCode {
    let info_file = PathBuf::from("target").join(BLOB_INFO_FILE);
    let mut info_in = match File::open(&info_file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open file '{}': {}", info_file.display(), e);
            return ExitCode::FAILURE;
        }
    };
    let blob_info = match read_blob_info(&mut info_in) {
	Ok(b) => b,
	Err(e) => {
            eprintln!("Failed to read file '{}': {}", info_file.display(), e);
	    return ExitCode::FAILURE;
	}
    };
    if let Err(e) = load_blob(&blob_info) {
        eprintln!("Failed to load blobs: {} ({:?})", e, e);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
