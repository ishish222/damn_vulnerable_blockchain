use std::path::PathBuf;
use std::fs;
use std::error::Error;

pub const ISHISH_HOME: &str = ".ishish";

pub async fn ensure_ishish_home(
    path: &PathBuf
) -> Result<(), Box<dyn Error>> {
    if !path.exists() {
        println!("Creating ishish home dir");
        fs::create_dir_all(&path).expect("Failed to create ishish home dir");
    }
    Ok(())
}