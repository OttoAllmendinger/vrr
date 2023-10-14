use anyhow::*;
use crate::image_loader::ImageRef;
use dirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const TAG_STARRED: &str = "starred";

pub struct Storage {
    path: PathBuf,
    cache: HashMap<PathBuf, ImageMetadata>,
}

#[derive(Serialize, Deserialize)]
pub struct ImageMetadata {
    pub tags: Vec<String>,
}

impl ImageMetadata {
    pub fn new() -> Self {
        Self { tags: vec![] }
    }

    pub fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }

    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.retain(|t| t != tag);
    }

    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&tag.to_string())
    }

    pub fn toggle_tag(&mut self, tag: String) {
        if self.has_tag(&tag) {
            self.remove_tag(&tag);
        } else {
            self.add_tag(tag);
        }
    }
}

impl Storage {
    pub fn new() -> Result<Self> {
        let mut config_dir = match dirs::config_dir() {
            Some(path) => path,
            None => PathBuf::from("."),
        };
        config_dir.push("vrr");

        // create dir if it does not exist
        if !config_dir.exists() {
            std::fs::create_dir_all(config_dir.clone())?;
        }

        let mut config_file = config_dir.clone();
        config_file.push("metadata.json");
        drop(config_dir);

        match std::fs::File::open(config_file.clone()) {
            Result::Ok(f) => {
                let cache: HashMap<PathBuf, ImageMetadata> = serde_json::from_reader(f)?;
                return Ok(Self { path: config_file, cache });
            }
            Err(_) => {
                let cache = HashMap::new();
                return Ok(Self { path: config_file, cache });
            }
        }
    }

    pub fn save(&self)  -> Result<()> {
        serde_json::to_writer(std::fs::File::create(self.path.clone())?, &self.cache)?;
        Ok(())
    }

    pub fn entry(&mut self, image_ref: &ImageRef) -> &mut ImageMetadata {
        let path = image_ref.path.clone();
        if !self.cache.contains_key(&path) {
            self.cache.insert(path.clone(), ImageMetadata::new());
        }
        self.cache.get_mut(&path).unwrap()
    }
}
