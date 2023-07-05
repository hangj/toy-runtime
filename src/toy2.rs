use std::{
    future::Future,
    pin::Pin,
    ptr::NonNull,
    sync::Arc,
    task::{Context, RawWaker, RawWakerVTable, Waker},
};

#[derive(Clone)]
struct Task {
    raw: RawTask,
}

unsafe impl Send for Task {}
unsafe impl Sync for Task {}

impl Task {
    fn poll(self) {
        let header = self.raw.header();
        unsafe {
            (header.vtable.poll_task)(self.raw.ptr);
        }
    }
}

struct RawTask {
    ptr: NonNull<Header>,
}

impl RawTask {
    fn header(&self) -> &Header {
        unsafe { self.ptr.as_ref() }
    }
}

impl Clone for RawTask {
    fn clone(&self) -> Self {
        let ptr = unsafe { (self.header().vtable.clone_task)(self.ptr) };
        Self { ptr }
    }
}

impl Drop for RawTask {
    fn drop(&mut self) {
        unsafe { (self.header().vtable.dealloc_task)(self.ptr) }
    }
}

struct Header {
    vtable: &'static Vtable,
    sender: crossbeam::channel::Sender<Task>,
}

/// repr(C) 确保指针 `*mut Cell<T>` 转换成 `*mut Header` 时有效, 
/// 因为默认情况下 Rust 的数据布局不一定会按照 field 的声明顺序排列
/// [The Default Representation](https://doc.rust-lang.org/reference/type-layout.html?#the-default-representation)
#[repr(C)]
struct Cell<T> {
    header: Header,
    future: T,
}

struct Vtable {
    poll_task: unsafe fn(NonNull<Header>),
    clone_task: unsafe fn(NonNull<Header>) -> NonNull<Header>,
    dealloc_task: unsafe fn(NonNull<Header>),
}

unsafe fn poll_task<T: Future>(ptr: NonNull<Header>) {
    let ptr = ptr.cast::<Cell<T>>().as_ptr();
    Arc::increment_strong_count(ptr);
    let waker = Waker::from_raw(RawWaker::new(
        ptr.cast(),
        &RawWakerVTable::new(
            clone_waker::<T>,
            wake_by_val::<T>,
            wake_by_ref::<T>,
            drop_waker::<T>,
        ),
    ));
    let mut cx = Context::from_waker(&waker);
    let pin = Pin::new_unchecked(&mut (*ptr).future);
    let _ = pin.poll(&mut cx);
}
unsafe fn clone_task<T>(ptr: NonNull<Header>) -> NonNull<Header> {
    let cell = ptr.cast::<Cell<T>>().as_ptr();
    Arc::increment_strong_count(cell);
    ptr
}
unsafe fn dealloc_task<T>(ptr: NonNull<Header>) {
    let ptr = ptr.cast::<Cell<T>>().as_ptr();
    Arc::decrement_strong_count(ptr);
}

unsafe fn clone_waker<T>(ptr: *const ()) -> RawWaker {
    let cell = ptr.cast::<Cell<T>>();
    Arc::increment_strong_count(cell);
    RawWaker::new(
        ptr,
        &RawWakerVTable::new(
            clone_waker::<T>,
            wake_by_val::<T>,
            wake_by_ref::<T>,
            drop_waker::<T>,
        ),
    )
}
unsafe fn drop_waker<T>(ptr: *const ()) {
    let ptr = ptr.cast::<Cell<T>>();
    Arc::decrement_strong_count(ptr);
}
unsafe fn wake_by_val<T>(ptr: *const ()) {
    let ptr = ptr.cast::<Cell<T>>();
    let raw = RawTask {
        ptr: NonNull::new_unchecked(ptr.cast::<Header>().cast_mut()),
    };
    let task = Task { raw };

    (*ptr).header.sender.send(task).unwrap();
}
unsafe fn wake_by_ref<T>(ptr: *const ()) {
    Arc::increment_strong_count(ptr.cast::<Cell<T>>());
    wake_by_val::<T>(ptr);
}

impl Task {
    fn new<T: Future>(future: T, sender: crossbeam::channel::Sender<Task>) -> Self {
        let header = Header {
            vtable: &Vtable {
                poll_task: poll_task::<T>,
                clone_task: clone_task::<T>,
                dealloc_task: dealloc_task::<T>,
            },
            sender,
        };
        let cell = Arc::into_raw(Arc::new(Cell { header, future }));
        let ptr = cell.cast::<Header>().cast_mut();

        Self {
            raw: RawTask {
                ptr: unsafe { NonNull::new_unchecked(ptr) },
            },
        }
    }
}

fn spawn<T: Future>(future: T, sender: crossbeam::channel::Sender<Task>) {
    let task = Task::new(future, sender.clone());
    sender.send(task).unwrap();
}

fn run(rx: crossbeam::channel::Receiver<Task>) {
    let mut threads = Vec::new();
    for _ in 0..4 {
        let rx = rx.clone();
        let th = std::thread::spawn(move || {
            while let Ok(task) = rx.recv() {
                task.poll();
            }
        });

        threads.push(th);
    }

    for th in threads {
        th.join().unwrap();
    }
}


#[cfg(test)]
mod tests {
    use crate::fake_io::FakeIO;

    use super::*;

    #[test]
    fn test() {
        let (tx, rx) = crossbeam::channel::unbounded::<Task>();

        for i in 0..10 {
            let sender = tx.clone();
            spawn(async move {
                let duration = FakeIO::new(std::time::Duration::from_secs(i)).await;
                println!("{}: {:?}", i, duration);
            }, sender);
        }

        drop(tx);

        run(rx);
    }
}