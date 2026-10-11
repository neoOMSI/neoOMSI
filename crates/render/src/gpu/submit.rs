//! The window picture's command buffers finished, submitted and presented on a thread of
//! their own, while the main thread goes on with the next frame's simulaton 

use std::sync::{Arc, Condvar, Mutex, mpsc};

pub(crate) struct SubmitJob {
    pub encoders: Vec<wgpu::CommandEncoder>,
    pub frame: Option<wgpu::SurfaceTexture>,
}

/// The thread's times, summed until the main thread collects them (OMSI_PROFILE).
#[derive(Default, Clone, Copy)]
pub(crate) struct SubmitTimes {
    pub finish: f64,
    pub submit: f64,
    pub present: f64,
}

struct Shared {
    busy: Mutex<usize>,
    done: Condvar,
    times: Mutex<SubmitTimes>,
}

pub(crate) struct Submitter {
    queue: wgpu::Queue,
    pool: Option<Arc<rayon::ThreadPool>>,
    tx: Option<mpsc::Sender<SubmitJob>>,
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Submitter {
    pub fn new(queue: wgpu::Queue, pool: Option<Arc<rayon::ThreadPool>>) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<SubmitJob>();
        let shared = Arc::new(Shared {
            busy: Mutex::new(0),
            done: Condvar::new(),
            times: Mutex::new(SubmitTimes::default()),
        });
        let worker = shared.clone();
        let (thread_queue, thread_pool) = (queue.clone(), pool.clone());
        let thread = std::thread::Builder::new()
            .name("render-submit".into())
            .spawn(move || {
                for job in rx {
                    // (a panic in the driver must not leave the main thread waiting)
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run(&thread_queue, thread_pool.as_deref(), job, &worker.times)
                    }));
                    let mut busy = worker.busy.lock().unwrap_or_else(|e| e.into_inner());
                    *busy = busy.saturating_sub(1);
                    worker.done.notify_all();
                }
            })
            .ok()?;
        Some(Self {
            queue,
            pool,
            tx: Some(tx),
            shared,
            thread: Some(thread),
        })
    }

    pub fn send(&self, job: SubmitJob) {
        *self.shared.busy.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        if let Some(Err(mpsc::SendError(job))) = self.tx.as_ref().map(|tx| tx.send(job)) {
            // the thread is gone: do it here
            *self.shared.busy.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
            run(&self.queue, self.pool.as_deref(), job, &self.shared.times);
        }
    }

    /// Waits until everything sent is submitted and presented; true when there was a wait.
    pub fn wait(&self) -> bool {
        let mut busy = self.shared.busy.lock().unwrap_or_else(|e| e.into_inner());
        let waited = *busy > 0;
        while *busy > 0 {
            busy = self
                .shared
                .done
                .wait(busy)
                .unwrap_or_else(|e| e.into_inner());
        }
        waited
    }

    pub fn take_times(&self) -> SubmitTimes {
        std::mem::take(&mut *self.shared.times.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl Drop for Submitter {
    fn drop(&mut self) {
        self.wait();
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Finishes the encoders in parallel when there are several (the pool keeps their order),
/// submits them in that order and presents the picture.
pub(crate) fn finish_all(
    pool: Option<&rayon::ThreadPool>,
    encoders: Vec<wgpu::CommandEncoder>,
) -> Vec<wgpu::CommandBuffer> {
    use rayon::prelude::*;
    match pool {
        Some(pool) if encoders.len() > 1 => {
            pool.install(|| encoders.into_par_iter().map(|e| e.finish()).collect())
        }
        _ => encoders.into_iter().map(|e| e.finish()).collect(),
    }
}

fn run(
    queue: &wgpu::Queue,
    pool: Option<&rayon::ThreadPool>,
    job: SubmitJob,
    times: &Mutex<SubmitTimes>,
) {
    let t0 = std::time::Instant::now();
    let commands = finish_all(pool, job.encoders);
    let t1 = std::time::Instant::now();
    queue.submit(commands);
    let t2 = std::time::Instant::now();
    if let Some(frame) = job.frame {
        queue.present(frame);
    }
    let t3 = std::time::Instant::now();
    let mut t = times.lock().unwrap_or_else(|e| e.into_inner());
    t.finish += (t1 - t0).as_secs_f64();
    t.submit += (t2 - t1).as_secs_f64();
    t.present += (t3 - t2).as_secs_f64();
}

impl crate::Renderer {
    /// Draws the window picture like `render`, but leaves its command buffers for `present`
    /// to finish and submit on the submit thread.
    pub fn render_deferred(
        &mut self,
        scene: &mut crate::Scene,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        camera: &crate::Camera,
        lighting: &crate::Lighting,
    ) {
        self.defer_next.set(self.submitter.is_some());
        self.render(scene, target, width, height, camera, lighting);
        self.defer_next.set(false);
    }

    /// While on, the pictures drawn into textures (the mirrors) leave their command buffers
    /// for the window picture's `present`, ahead of the picture's own.
    pub fn set_defer_views(&self, on: bool) {
        self.defer_views.set(on && self.submitter.is_some());
    }

    /// Commands for textures the window picture shows (the navigator's panel): submitted
    /// just before the picture's own, finished on the submit thread with them.
    pub fn submit_with_frame(&self, encoder: wgpu::CommandEncoder) {
        self.queued.borrow_mut().push(encoder);
    }

    /// Presents the window picture: what `render_deferred`, the mirrors and
    /// `submit_with_frame` left is finished, submitted and presented on the submit thread.
    pub fn present(&self, frame: wgpu::SurfaceTexture) {
        let queued = std::mem::take(&mut *self.queued.borrow_mut());
        match self.submitter.as_ref() {
            Some(s) => s.send(SubmitJob {
                encoders: queued,
                frame: Some(frame),
            }),
            None => {
                if !queued.is_empty() {
                    self.queue
                        .submit(finish_all(self.encoding_pool.as_deref(), queued));
                }
                self.queue.present(frame);
            }
        }
    }

    /// Submits what was left for `present` now, on this thread (something else is drawn over
    /// the picture, or there is no window picture to present).
    pub fn flush_deferred(&self) {
        let queued = std::mem::take(&mut *self.queued.borrow_mut());
        if !queued.is_empty() {
            self.queue
                .submit(finish_all(self.encoding_pool.as_deref(), queued));
        }
    }

    pub(crate) fn has_queued(&self) -> bool {
        !self.queued.borrow().is_empty()
    }

    /// Waits until the submit thread has submitted (and presented) what it was sent.
    pub fn wait_submitted(&self) {
        let Some(s) = self.submitter.as_ref() else {
            return;
        };
        let t = std::time::Instant::now();
        let waited = s.wait();
        if self.profiling {
            let times = s.take_times();
            let mut stats = self.stats.borrow_mut();
            if waited {
                *stats.entry("wait submit").or_default() += t.elapsed().as_secs_f64();
            }
            *stats.entry("thread.finish").or_default() += times.finish;
            *stats.entry("thread.submit").or_default() += times.submit;
            *stats.entry("thread.present").or_default() += times.present;
        }
    }
}
