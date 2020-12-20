use std::fs::{self, create_dir, File};
use std::io::prelude::*;
use std::io::ErrorKind;
use std::path::PathBuf;

/// Simple file backed cache
pub struct FileCache {}

impl FileCache {
    const CACHE_DIR: &'static str = "./cache";

    pub fn new() -> Result<Self, std::io::Error> {
        match create_dir(Self::CACHE_DIR) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        Ok(FileCache {})
    }

    fn get_cache_file(key: &str) -> PathBuf {
        PathBuf::from(&format!("{}/{}", Self::CACHE_DIR, key))
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, std::io::Error> {
        let mut file = match File::open(Self::get_cache_file(key)) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };

        // TODO: remove if file too old? if so possible race
        // can use an rwlock in that case

        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(Some(contents))
    }

    // TODO: make insert mutably borrow
    pub fn insert(&self, key: &str, value: &str) -> Result<(), std::io::Error> {
        // create_new is atomic, so possible race condition should be avoided
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(Self::get_cache_file(key))
        {
            Ok(f) => f,
            // TODO: what if the file already exists
            // remove it and overwrite?
            Err(e) if e.kind() == ErrorKind::AlreadyExists => return Ok(()),
            Err(e) => return Err(e),
        };

        file.write_all(value.as_bytes())
    }
}
