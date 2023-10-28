use crate::texture::{ImageResolution, SizedImage};
use anyhow::*;
use log::{debug, error};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, SendError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageRef {
    pub path: PathBuf,
}

impl ImageRef {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageRequest {
    pub reference: ImageRef,
    pub resolution: ImageResolution,
}

impl ImageRequest {
    pub fn new(reference: ImageRef, resolution: ImageResolution) -> Self {
        Self {
            reference,
            resolution,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum LoadState {
    Pending,
    Loaded,
}

pub struct ImageLoader {
    pub images: Vec<ImageRef>,
    pub sender: Sender<Result<SizedImage>>,
    pub receiver: Receiver<Result<SizedImage>>,
    pub preload: usize,
    index: usize,
    cache: Arc<Mutex<HashMap<ImageRequest, LoadState>>>,
    thread_pool: rayon::ThreadPool,
}

impl ImageLoader {
    pub fn from_paths(paths: Vec<PathBuf>, preload: usize) -> Self {
        let mut images = Vec::new();
        for path in paths {
            images.push(ImageRef::new(path));
        }
        let (sender, receiver) = channel();
        let num_threads = thread::available_parallelism()
            .unwrap_or(NonZeroUsize::new(2).unwrap())
            .get();
        debug!("Creating thread pool with {} threads", num_threads);
        let mut loader = Self {
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap(),
            preload,
            cache: Arc::new(Mutex::new(HashMap::new())),
            sender,
            receiver,
            images,
            index: 0,
        };
        if loader.len() > 0 {
            loader.set(0).unwrap();
            // log and discard error
            /*
            loader
                .load_all_thumbnails()
                .map_err(|e| error!("Error loading thumbnails: {}", e))
                .ok();

             */
        }
        loader
    }

    pub fn from_path(path: PathBuf, preload: usize) -> Result<Self> {
        if path.is_file() {
            let mut dir = path.clone();
            dir.pop();
            let mut loader = Self::from_path(dir, preload)?;
            loader.set(loader.images.iter().position(|p| p.path == path).unwrap())?;
            return Ok(loader);
        }

        let mut paths = Vec::new();
        for entry in std::fs::read_dir(path.clone())
            .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {}", path.display(), e)))?
        {
            let entry = entry.unwrap();
            let path = entry.path();
            let is_jpeg = path
                .extension()
                .map(|e| e.to_ascii_lowercase())
                .map(|e| e == "jpg" || e == "jpeg")
                .unwrap_or(false);
            if path.is_file() && is_jpeg {
                paths.push(path)
            }
        }
        paths.sort();
        Ok(Self::from_paths(paths, preload))
    }

    pub fn current(&self) -> ImageRef {
        self.get(self.index).unwrap().clone()
    }

    pub fn get(&self, index: usize) -> Result<&ImageRef> {
        self.images
            .get(index)
            .ok_or(anyhow!("No image at index {}", index))
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn set(&mut self, index: usize) -> Result<()> {
        self.index = index;
        self.request_image(&ImageRequest::new(
            self.get(index)?.clone(),
            ImageResolution::NATIVE,
        ));
        Ok(())
    }

    pub fn preload(&mut self, preload: usize) -> Result<()> {
        for iref in self.get_radius(preload) {
            self.request_image(&ImageRequest::new(iref.clone(), ImageResolution::NATIVE));
        }

        Ok(())
    }

    pub fn next_image(&mut self) -> Result<()> {
        self.set((self.index + 1) % self.len())?;
        Ok(())
    }

    pub fn prev_image(&mut self) -> Result<()> {
        self.set((self.index + self.len() - 1) % self.len())?;
        Ok(())
    }

    pub fn request_image(&mut self, req: &ImageRequest) {
        let mut cache = self.cache.lock().unwrap();
        if cache.get(req).is_some() {
            // already requested
            return;
        }
        let n_pending = cache
            .iter()
            .filter(|(req, state)| {
                **state == LoadState::Pending && req.resolution != ImageResolution::THUMBNAIL
            })
            .count();
        cache.insert(req.clone(), LoadState::Pending);
        let sender = self.sender.clone();
        let req = req.clone();
        let cache = Arc::clone(&self.cache);
        self.thread_pool.spawn(move || {
            // debounce to avoid spamming the thread pool
            // if user is scrolling quickly, the requests might be expired after this delay
            if n_pending > 0 {
                let delay_ms = if n_pending > 5 { 100 } else { 10 };
                thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            if cache.lock().unwrap().get(&req).is_none() {
                return;
            }
            let sized_image = SizedImage::from_request(req.clone());
            if let Err(SendError(_)) = sender.send(sized_image) {
                debug!("send error: {:?}", req);
                return;
            }
            cache.lock().unwrap().insert(req, LoadState::Loaded);
        })
    }

    pub fn load_all_thumbnails(&mut self) -> Result<()> {
        for i in 0..self.len() {
            let iref = self.get(i)?.clone();
            self.request_image(&ImageRequest::new(iref, ImageResolution::THUMBNAIL));
        }
        Ok(())
    }

    fn get_offset(&self, offset: isize) -> Result<&ImageRef> {
        let index = (self.len() as isize + self.index as isize + offset) % self.len() as isize;
        self.get(index as usize)
    }

    fn get_radius(&self, radius: usize) -> Vec<ImageRef> {
        let radius = radius as isize;
        let mut irefs = vec![self.get_offset(0).unwrap().clone()];
        for i in 1..=radius {
            for iref in [self.get_offset(i), self.get_offset(-i)] {
                if let Result::Ok(iref) = iref {
                    irefs.push(iref.clone());
                }
            }
        }
        irefs
    }

    pub fn clear_cache(&mut self) {
        let keep = self.get_radius(self.preload);
        self.cache.lock().unwrap().retain(|req, _| {
            keep.contains(&&req.reference) || req.resolution == ImageResolution::THUMBNAIL
        });
    }

    pub fn cached(&self) -> Vec<ImageRequest> {
        self.cache.lock().unwrap().keys().cloned().collect()
    }

    pub fn images(&mut self) -> Vec<SizedImage> {
        let mut images = Vec::new();
        while let Result::Ok(res) = self.receiver.try_recv().map_err(|e| anyhow!(e)) {
            match res {
                Result::Ok(image) => images.push(image),
                Err(e) => error!("Error loading image: {}", e),
            }
        }
        images
    }
}
