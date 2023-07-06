use std::{
    collections::VecDeque,
    mem,
    ptr::NonNull,
    sync::{Arc, Condvar, Mutex},
};

struct MessageQueue {
    queue: Mutex<VecDeque<NonNull<()>>>,
    cond: Condvar,
}

impl MessageQueue {
    pub fn new() -> Self {
        Self {
            cond: Condvar::new(),
            queue: Mutex::new(VecDeque::new()),
        }
    }

    pub fn recv(&mut self) -> NonNull<()> {
        let mut queue = self.queue.lock().unwrap();
        if queue.is_empty() {
            let queue = self.cond.wait(queue);
            queue.unwrap().pop_front().unwrap()
        } else {
            queue.pop_front().unwrap()
        }
    }
}

pub struct Mq {
    mutex: *mut libc::pthread_mutex_t,
    cond: *mut libc::pthread_cond_t,
    queue: *mut VecDeque<NonNull<()>>,
}

unsafe impl Send for Mq {}
unsafe impl Sync for Mq {}

impl Drop for Mq {
    fn drop(&mut self) {
        unsafe {
            libc::pthread_mutex_destroy(self.mutex);
            libc::pthread_cond_destroy(self.cond);
            let _ = Box::from_raw(self.mutex);
            let _ = Box::from_raw(self.cond);
            let _ = Box::from_raw(self.queue);
        }
    }
}

#[derive(Clone)]
pub struct Sender(Arc<Mq>);

#[derive(Clone)]
pub struct Receiver(Arc<Mq>);

impl Sender {
    pub fn send(&self, msg: NonNull<()>) {
        self.0.send(msg);
    }
}

impl Receiver {
    pub fn recv(&self) -> NonNull<()> {
        self.0.recv()
    }
}

impl Mq {
    pub fn new() -> Arc<Self> {
        let mutex: libc::pthread_mutex_t = unsafe { mem::zeroed() };
        let cond: libc::pthread_cond_t = unsafe { mem::zeroed() };

        let mutex = Box::into_raw(Box::new(mutex));
        let cond = Box::into_raw(Box::new(cond));
        let queue = VecDeque::new();
        let queue = Box::into_raw(Box::new(queue));

        unsafe {
            if 0 != libc::pthread_mutex_init(mutex, std::ptr::null()) {
                panic!("pthread_mutex_init failed");
            }
            if 0 != libc::pthread_cond_init(cond, std::ptr::null()) {
                panic!("pthread_cond_init failed");
            }
        }
        Arc::new(Self { mutex, cond, queue })
    }
    pub fn split(self: &Arc<Self>) -> (Sender, Receiver) {
        (Sender(self.clone()), Receiver(self.clone()))
    }
    pub fn send(self: &Arc<Self>, msg: NonNull<()>) {
        self.lock();
        let queue = Box::leak(unsafe { Box::from_raw(self.queue) });
        queue.push_back(msg);

        // unsafe { libc::pthread_cond_broadcast(&mut self.cond) };
        unsafe { libc::pthread_cond_signal(self.cond) };

        self.unlock();
    }
    pub fn recv(self: &Arc<Self>) -> NonNull<()> {
        let queue = Box::leak(unsafe { Box::from_raw(self.queue) });
        loop {
            self.lock();

            if let Some(future) = queue.pop_front() {
                self.unlock();

                return future;
            }

            unsafe {
                libc::pthread_cond_wait(self.cond, self.mutex);
            }
            self.unlock();
        }
    }

    #[inline]
    fn lock(self: &Arc<Self>) {
        unsafe {
            libc::pthread_mutex_lock(self.mutex);
        }
    }
    #[inline]
    fn unlock(self: &Arc<Self>) {
        unsafe {
            libc::pthread_mutex_unlock(self.mutex);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test() {
        let mq = Mq::new();

        let mut threads = Vec::new();

        for i in 0..4 {
            let mq_cloned = mq.clone();

            let th = std::thread::spawn(move || {
                let mq = mq_cloned;
                loop {
                    println!("thread({}) wait popping", i);
                    let msg = mq.recv();
                    let boxed = unsafe { Box::from_raw(msg.cast::<i32>().as_ptr()) };
                    println!("thread({}) got boxed {}", i, *boxed);

                    let rand = unsafe { libc::rand() }.abs_diff(1) % 5;
                    std::thread::sleep(Duration::from_secs(rand as u64));
                }
            });

            threads.push(th);
        }

        for i in 0..50 {
            let msg = unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(i))) };
            mq.send(msg.cast());
        }

        for th in threads {
            let _ = th.join();
        }
    }
}
