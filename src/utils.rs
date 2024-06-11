use std::{
    path::PathBuf,
    error::Error,
    env,
    fs
};

use libp2p::gossipsub::{self, IdentTopic};

pub const ISHISH_HOME: &str = ".ishish";
pub const ISHISH_TOPIC: &str = "test-net";
pub const DEFAULT_DIFFICULTY: u32 = 2;

pub async fn ensure_ishish_home(
) -> Result<PathBuf, Box<dyn Error>> {
    /* setup wallet dir path */
    let mut path = PathBuf::new();
    let home_dir = env::var_os("HOME").expect("HOME is not set in env.");
    path.push(home_dir);
    path.push(ISHISH_HOME);

    if !path.exists() {
        println!("Creating ishish home dir");
        fs::create_dir_all(&path).expect("Failed to create ishish home dir");
    }
    Ok(path)
}