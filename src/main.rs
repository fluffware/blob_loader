use blob_loader::build_blob;

fn main()
{
    
    if let Err(e) = build_blob::prepare_blob() {
        eprintln!("Failed to prepare blob: {}", e);
    }
}
