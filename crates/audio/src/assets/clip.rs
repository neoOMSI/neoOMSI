//! Decoded sound clips and the cache that keeps them: the clip type itself, reading a file,
//! and letting go of clips nobody uses any more (see [`ClipCache::trim`]).

use hashbrown::{HashMap, HashSet};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Clip {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved 16-bit samples (see `wav::WavData`).
    pub samples: Vec<i16>,
}

impl Clip {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
}

/// Read and decode a clip (any thread).
pub fn read_clip(path: &Path) -> Option<Arc<Clip>> {
    let bytes = ::legacy_config::vfs::read(path).ok()?;
    match crate::assets::wav::parse_wav(&bytes) {
        Ok(w) => Some(Arc::new(Clip {
            sample_rate: w.sample_rate,
            channels: w.channels,
            samples: w.samples,
        })),
        Err(e) => {
            log::warn!("{}: {e}", path.display());
            None
        }
    }
}

/// Clips by file, with when each was last asked for. `None` stands for a file that is
/// missing or unreadable (it is not tried again). The cache is shared with the background
/// reader (see [`ClipCache::ready`]), so it is held behind an `Arc`.
pub struct ClipCache {
    clips: Mutex<HashMap<PathBuf, (Option<Arc<Clip>>, Instant)>>,
    last_trim: Mutex<Instant>,
    /// Files a background reader is working on.
    loading: Mutex<HashSet<PathBuf>>,
}

impl ClipCache {
    pub fn new(now: Instant) -> ClipCache {
        ClipCache {
            clips: Mutex::new(HashMap::new()),
            last_trim: Mutex::new(now),
            loading: Mutex::new(HashSet::new()),
        }
    }

    /// Put an already decoded clip into the cache under `path`: later [`ClipCache::load`]
    /// calls for it return this one instead of reading a file. Tests and the offline renderer
    /// use it to supply synthetic sounds for a sound set.
    pub fn insert(&self, path: impl Into<PathBuf>, clip: Arc<Clip>) {
        self.clips
            .lock()
            .insert(path.into(), (Some(clip), Instant::now()));
    }

    /// A clip from the cache, read now if it is not there.
    pub fn load(&self, path: &Path) -> Option<Arc<Clip>> {
        if let Some(c) = self.clips.lock().get_mut(path) {
            c.1 = Instant::now();
            return c.0.clone();
        }
        let clip = read_clip(path);
        if clip.is_none() {
            let dir = path.parent().unwrap_or(path);
            ::mods::diagnostics::missing(dir, path, ::mods::diagnostics::Kind::Sound);
        }
        self.clips
            .lock()
            .insert(path.to_path_buf(), (clip.clone(), Instant::now()));
        clip
    }

    /// Let go of the clips nobody holds (no sound set, no voice) and nobody asked for in
    /// `unused`, once every ten seconds at most: the sounds of every vehicle that ever came
    /// into earshot stayed (140 MB around the Ahlheim main station). They are read again in
    /// the background when a vehicle comes back. Returns the bytes let go.
    pub fn trim(&self, unused: Duration) -> usize {
        {
            let mut last = self.last_trim.lock();
            if last.elapsed().as_secs_f32() < 10.0 {
                return 0;
            }
            *last = Instant::now();
        }
        let mut freed = 0usize;
        self.clips.lock().retain(|_, (clip, used)| {
            let idle = used.elapsed() >= unused
                && clip
                    .as_ref()
                    .map(|c| Arc::strong_count(c) == 1)
                    .unwrap_or(false);
            if idle {
                freed += clip.as_ref().map(|c| c.samples.len() * 2).unwrap_or(0);
            }
            !idle
        });
        freed
    }

    /// Whether all of `paths` are in the cache (or known to be missing). The ones that are
    /// not are read on a background thread, started on the first call: a vehicle coming
    /// into earshot for the first time had its sounds read in the frame, which on a map
    /// read from an archive was a stall of up to 400 ms.
    pub fn ready(self: &Arc<Self>, paths: &[PathBuf], enabled: bool) -> bool {
        if !enabled {
            return true;
        }
        let missing: Vec<PathBuf> = {
            let mut clips = self.clips.lock();
            let now = Instant::now();
            paths
                .iter()
                .filter(|p| match clips.get_mut(*p) {
                    Some(c) => {
                        c.1 = now;
                        false
                    }
                    None => true,
                })
                .cloned()
                .collect()
        };
        if missing.is_empty() {
            return true;
        }
        let todo: Vec<PathBuf> = {
            let mut loading = self.loading.lock();
            missing
                .into_iter()
                .filter(|p| loading.insert(p.clone()))
                .collect()
        };
        if !todo.is_empty() {
            let cache = Arc::clone(self);
            let back = todo.clone();
            let spawned = std::thread::Builder::new()
                .name("sound loader".into())
                .spawn(move || {
                    for p in todo {
                        let c = read_clip(&p);
                        cache.clips.lock().insert(p.clone(), (c, Instant::now()));
                        cache.loading.lock().remove(&p);
                    }
                });
            if spawned.is_err() {
                // no thread: read them here
                for p in back {
                    self.load(&p);
                    self.loading.lock().remove(&p);
                }
                return true;
            }
        }
        false
    }
}
