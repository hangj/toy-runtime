use std::{
    future::Future, marker::PhantomData, pin::Pin, ptr::NonNull, sync::{Arc, Mutex}, task::{Context, Poll, RawWaker, RawWakerVTable, Waker}
};

pub struct Handle<Out> {
    ptr: NonNull<Header>,
    _unused: PhantomData<Out>,
}

impl<Out> Handle<Out> {
    pub fn read_output(&mut self) -> Option<Out> {
        let header = unsafe{self.ptr.as_mut()};

        let mut out: Option<Out> = None;
        let dst = &mut out;
        unsafe {
            (header.vtable.read_output)(self.ptr, dst as *mut _ as _)
        }

        out
    }
}

impl<Out> Drop for Handle<Out> {
    fn drop(&mut self) {
        unsafe {
            (self.ptr.as_ref().vtable.drop_task)(self.ptr);
        }
    }
}

struct Task {
    raw: RawTask,
}

unsafe impl Send for Task {}
unsafe impl Sync for Task {}

struct RawTask {
    ptr: NonNull<Header>, // pointer to Cell<T> where T: Future
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
        unsafe { (self.header().vtable.drop_task)(self.ptr) }
    }
}

struct Header {
    // todo: maybe replace the Mutex<State> with AtomicUsize
    state: Mutex<State>,
    vtable: &'static Vtable,
    sender: crossbeam::channel::Sender<Task>,
}

#[derive(Default)]
struct State {
    running: bool,
    notified: bool,
    completed: bool,
}

/// #[repr(C)] make sure `*mut Cell<T>` can cast to valid `*mut Header`, and backwards. 
/// In the default situation, the data layout may not be the same as the order in which the fields are specified in the declaration of the type
/// 默认情况下 Rust 的数据布局不一定会按照 field 的声明顺序排列
/// [The Default Representation](https://doc.rust-lang.org/reference/type-layout.html?#the-default-representation)
///
/// [playground link](https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=39ac84782d121970598b91201b168f82)
/// 
/// you can easilly view the data layout with this crate https://github.com/hangj/layout-rs
#[repr(C)]
struct Cell<T: Future> {
    header: Header,
    future: T,
    output: Option<T::Output>,
}

struct Vtable {
    poll_task: unsafe fn(NonNull<Header>),
    clone_task: unsafe fn(NonNull<Header>) -> NonNull<Header>,
    drop_task: unsafe fn(NonNull<Header>),
    read_output: unsafe fn(NonNull<Header>, *mut ()),
}

unsafe fn poll_task<T: Future>(non_null: NonNull<Header>) {
    let ptr = non_null.cast::<Cell<T>>().as_ptr();
    let state = &(*ptr).header.state;
    {
        // The future can be waked by multiple wakers, so this task may be polled 
        // by more than one thread at the same time
        let mut state = state.lock().unwrap();
        if state.completed {
            return;
        }
        if state.running {
            state.notified = true;
            return;
        }
        state.running = true;
        state.notified = false;
    }

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

    let is_completed = if let Poll::Ready(output) = pin.poll(&mut cx) {
        (*ptr).output = Some(output);
        true
    } else {
        false
    };

    {
        // if the task is polled again when running, we just need to send it back to the channel
        let mut state = state.lock().unwrap();
        state.running = false;
        if is_completed {
            state.completed = true;
            return;
        }
        if state.notified {
            drop(state);
            // send the task
            Arc::increment_strong_count(ptr);
            let task = Task {
                raw: RawTask { ptr: non_null },
            };
            (*ptr).header.sender.send(task).unwrap();
        }
    }
}
unsafe fn clone_task<T: Future>(ptr: NonNull<Header>) -> NonNull<Header> {
    let cell = ptr.cast::<Cell<T>>().as_ptr();
    Arc::increment_strong_count(cell);
    ptr
}
unsafe fn drop_task<T: Future>(ptr: NonNull<Header>) {
    let ptr = ptr.cast::<Cell<T>>().as_ptr();
    Arc::decrement_strong_count(ptr);
}
unsafe fn read_output<T:Future>(ptr: NonNull<Header>, dst: *mut ()) {
    let cell = ptr.cast::<Cell<T>>().as_mut();
    let out = &mut *(dst as *mut Option<T::Output>);
    *out = cell.output.take();
}

unsafe fn clone_waker<T: Future>(ptr: *const ()) -> RawWaker {
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
unsafe fn drop_waker<T: Future>(ptr: *const ()) {
    let ptr = ptr.cast::<Cell<T>>();
    Arc::decrement_strong_count(ptr);
}
unsafe fn wake_by_val<T: Future>(ptr: *const ()) {
    let ptr = ptr.cast::<Cell<T>>();
    let raw = RawTask {
        ptr: NonNull::new_unchecked(ptr.cast::<Header>().cast_mut()),
    };
    let task = Task { raw };

    (*ptr).header.sender.send(task).unwrap();
}
unsafe fn wake_by_ref<T: Future>(ptr: *const ()) {
    Arc::increment_strong_count(ptr.cast::<Cell<T>>());
    wake_by_val::<T>(ptr);
}

impl Task {
    fn new<T: Future>(future: T, sender: crossbeam::channel::Sender<Task>) -> Self {
        let header = Header {
            state: Mutex::new(State::default()),
            vtable: &Vtable {
                poll_task: poll_task::<T>,
                clone_task: clone_task::<T>,
                drop_task: drop_task::<T>,
                read_output: read_output::<T>,
            },
            sender,
        };
        let cell = Arc::into_raw(Arc::new(Cell {
            header,
            future,
            output: None,
        }));
        let ptr = cell.cast::<Header>().cast_mut();

        Self {
            raw: RawTask {
                ptr: unsafe { NonNull::new_unchecked(ptr) },
            },
        }
    }

    fn poll(self) {
        let header = self.raw.header();
        unsafe {
            (header.vtable.poll_task)(self.raw.ptr);
        }
    }
}

fn spawn<T: Future>(future: T, sender: crossbeam::channel::Sender<Task>) -> Handle<T::Output> {
    let task = Task::new(future, sender.clone());

    let ptr = unsafe { (task.raw.header().vtable.clone_task)(task.raw.ptr) };
    let handle = Handle {
        ptr,
        _unused: PhantomData,
    };

    sender.send(task).unwrap();

    handle
}

fn run(rx: crossbeam::channel::Receiver<Task>, nthread: usize) {
    let mut threads = Vec::new();
    for _ in 0..nthread {
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

pub struct Toy {
    tx: crossbeam::channel::Sender<Task>,
    rx: crossbeam::channel::Receiver<Task>,
}

impl Toy {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam::channel::bounded::<Task>(1024);
        Self { tx, rx }
    }
    pub fn spawn<T: Future>(&self, future: T) -> Handle<T::Output> {
        spawn(future, self.tx.clone())
    }
    pub fn run(self, nthread: usize) {
        drop(self.tx);
        run(self.rx, nthread);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_io::FakeIO;

    #[test]
    fn test_toy() {
        let toy = Toy::new();
        for i in 1..=20 {
            toy.spawn(async move {
                let duration = FakeIO::new(std::time::Duration::from_secs(i)).await;
                println!("{}: {:?}", i, duration);
            });
        }
        toy.run(4);
    }

    #[test]
    fn test() {
        let nfuture = 100;
        let nthread = 4;

        let (tx, rx) = crossbeam::channel::unbounded::<Task>();

        for i in 0..nfuture {
            let sender = tx.clone();
            spawn(
                async move {
                    let duration = FakeIO::new(std::time::Duration::from_secs(i)).await;
                    println!("{}: {:?}", i, duration);
                },
                sender,
            );
        }

        drop(tx);

        run(rx, nthread);
    }
}
