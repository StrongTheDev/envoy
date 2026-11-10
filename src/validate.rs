//! Background check that folders referenced by variables exist.

use crate::model::path_key;
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

struct Job {
    rx: Receiver<(String, bool)>,
    total: usize,
    done: usize,
}

#[derive(Default)]
pub struct Validator {
    /// `path_key` → exists
    cache: HashMap<String, bool>,
    job: Option<Job>,
}

impl Validator {
    /// Make sure every path in `paths` gets checked. Already-known paths are skipped.
    pub fn request(&mut self, paths: &[String], ctx: &egui::Context) {
        let mut todo: Vec<String> = Vec::new();
        for p in paths {
            let key = path_key(p);
            if !self.cache.contains_key(&key) && !todo.contains(&key) {
                todo.push(key);
            }
        }
        if todo.is_empty() {
            return;
        }

        // Replacing the job drops the old receiver, which stops the old thread.
        let (tx, rx) = channel();
        self.job = Some(Job { rx, total: todo.len(), done: 0 });
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for key in todo {
                let exists = Path::new(&key).exists();
                if tx.send((key, exists)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }

    /// Collect finished checks. Returns true when anything new arrived.
    pub fn poll(&mut self) -> bool {
        let Some(job) = &mut self.job else { return false };
        let mut changed = false;
        while let Ok((key, exists)) = job.rx.try_recv() {
            self.cache.insert(key, exists);
            job.done += 1;
            changed = true;
        }
        if job.done >= job.total {
            self.job = None;
            changed = true;
        }
        changed
    }

    /// `(done, total)` while a pass is running.
    pub fn progress(&self) -> Option<(usize, usize)> {
        self.job.as_ref().map(|j| (j.done, j.total))
    }

    pub fn exists(&self, path: &str) -> Option<bool> {
        self.cache.get(&path_key(path)).copied()
    }

    /// Forget everything so the next request re-checks the disk.
    pub fn reset(&mut self) {
        self.cache.clear();
        self.job = None;
    }
}
