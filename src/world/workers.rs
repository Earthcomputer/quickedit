use std::collections::VecDeque;
use std::ops::Deref;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use glam::{IVec2, Vec3Swizzles};
use lazy_static::lazy_static;
use crate::fname::FName;
use crate::geom::{IVec2Extensions, IVec2RangeExtensions};
use crate::{geom, World};

pub struct WorldRef {
    thread_pool: rayon::ThreadPool,
    world: Arc<World>,
    dropping: Arc<AtomicBool>,
    num_jobs_semaphore: Arc<(Mutex<usize>, Condvar)>,
}
unsafe impl Send for WorldRef {}
unsafe impl Sync for WorldRef {}
impl WorldRef {
    pub(super) fn new(world: World) -> Self {
        let world_name = world.path.file_name().and_then(|str| str.to_str()).unwrap_or("<unnamed world>").to_string();
        Self {
            thread_pool: rayon::ThreadPoolBuilder::new()
                .thread_name(move |idx| format!("WorldWorker-{}-{}", world_name, idx))
                .spawn_handler(|thread| {
                    let mut b = std::thread::Builder::new();
                    if let Some(name) = thread.name() {
                        b = b.name(name.to_owned());
                    }
                    if let Some(stack_size) = thread.stack_size() {
                        b = b.stack_size(stack_size);
                    }
                    b.spawn(|| {
                        thread.run();
                    })?;
                    Ok(())
                })
                .num_threads((num_cpus::get() - 1).max(4))
                .build().unwrap(),
            world: Arc::new(world),
            dropping: Arc::new(AtomicBool::new(false)),
            num_jobs_semaphore: Arc::new((Mutex::new(0), Condvar::new())),
        }
    }

    /// Please kindly only access the world from your worker, and not abuse your 'static lifetime :)
    pub fn spawn_worker<F>(&self, job: F)
        where
            F: FnOnce(Arc<World>, &dyn Fn() -> bool) + Send + 'static,
    {
        struct Guard {
            num_jobs_semaphore: Arc<(Mutex<usize>, Condvar)>,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                *self.num_jobs_semaphore.0.lock().unwrap() -= 1;
                self.num_jobs_semaphore.1.notify_one();
            }
        }
        let num_jobs_guard = Guard{num_jobs_semaphore: self.num_jobs_semaphore.clone()};
        *self.num_jobs_semaphore.0.lock().unwrap() += 1;

        let world = self.world.clone();
        let dropping = self.dropping.clone();
        self.thread_pool.spawn(move || {
            let _num_jobs_guard = num_jobs_guard;
            if !dropping.load(Ordering::Relaxed) {
                job(world, &(|| dropping.load(Ordering::Relaxed)));
            }
        });
    }
}
impl Deref for WorldRef {
    type Target = World;
    fn deref(&self) -> &World {
        &self.world
    }
}
impl Drop for WorldRef {
        fn drop(&mut self) {
        self.dropping.store(true, Ordering::Relaxed);
        drop(self.num_jobs_semaphore.1.wait_while(self.num_jobs_semaphore.0.lock().unwrap(), |num_jobs| *num_jobs > 0).unwrap());
    }
}

lazy_static! {
    static ref GLOBAL_TICK_VAR: Condvar = Condvar::new();
    static ref GLOBAL_TICK_MUTEX: Mutex<usize> = Mutex::new(0);
}

pub fn tick() {
    {
        let mut global_tick_mutex = GLOBAL_TICK_MUTEX.lock().unwrap();
        *global_tick_mutex = global_tick_mutex.wrapping_add(1);
    }
    GLOBAL_TICK_VAR.notify_all();
}

pub fn worker_yield() {
    drop(GLOBAL_TICK_VAR.wait(GLOBAL_TICK_MUTEX.lock().unwrap()).unwrap());
}

pub(super) fn chunk_loader(world: Arc<World>, stop: &dyn Fn() -> bool) {
    let render_distance = crate::get_config().render_distance() as i32;
    let mut prev_dimension: Option<FName> = None;
    let mut prev_chunk_pos: Option<IVec2> = None;
    let mut chunks_to_unload = VecDeque::new();

    'outer_loop:
    while !stop() {
        // find chunk to load
        let (dimension, pos) = {
            let camera = world.camera.read().unwrap();
            (camera.dimension.clone(), camera.pos)
        };
        if prev_dimension.as_ref() != Some(&dimension) {
            if let Some(prev_dimension) = prev_dimension {
                for chunk_pos in prev_chunk_pos.unwrap().square_range(render_distance).iter() {
                    chunks_to_unload.push_back((prev_dimension.clone(), chunk_pos));
                }
            }
            prev_dimension = Some(dimension.clone());
        }

        let chunk_pos = pos.xz().floor().as_ivec2() >> 4;

        if prev_chunk_pos != Some(chunk_pos) {
            if let Some(prev_chunk_pos) = prev_chunk_pos {
                for cp in prev_chunk_pos.square_range(render_distance).iter() {
                    let delta: IVec2 = cp - chunk_pos;
                    let distance = delta.abs().max_element();
                    if distance > render_distance {
                        chunks_to_unload.push_back((dimension.clone(), cp));
                    }
                }
            }
            prev_chunk_pos = Some(chunk_pos);
        }

        while !chunks_to_unload.is_empty() {
            let (dimension, chunk_pos) = chunks_to_unload.pop_front().unwrap();
            if let Some(dimension) = world.get_dimension(&dimension) {
                if dimension.unload_chunk(&world, chunk_pos) {
                    break;
                }
            }
        }

        let dimension = match world.get_dimension(&dimension) {
            Some(dimension) => dimension,
            None => {
                worker_yield();
                continue
            },
        };

        for chunk_pos in geom::iter_diamond_within_square(chunk_pos, render_distance) {
            if dimension.get_chunk(chunk_pos).is_none()
                && dimension.does_chunk_exist(&world, chunk_pos)
                && dimension.load_chunk(&world, chunk_pos).is_some()
            {
                continue 'outer_loop;
            }
        }

        worker_yield();
    }
}