use std::{
    future::Future,
    pin::Pin,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Mutex,
    },
    task::{Context, RawWaker, RawWakerVTable, Waker}, mem::ManuallyDrop,
};

pub struct Executor {
    receiver: Receiver<Arc<Task>>,
}

pub struct Spawner {
    sender: SyncSender<Arc<Task>>,
}

struct Task {
    future: Mutex<Pin<Box<dyn Future<Output = ()>>>>,
    sender: SyncSender<Arc<Task>>,
}

impl Task {
    fn wake(self: Arc<Self>) {
        self.sender.send(self.clone()).expect("receiver is disconnected or too many tasks queued");
    }
}

pub fn new_executor_and_spawner() -> (Executor, Spawner) {
    const MAX_QUEUED_TASKS: usize = 10_000;
    let (sender, receiver) = sync_channel(MAX_QUEUED_TASKS);
    (Executor { receiver }, Spawner { sender })
}

impl Spawner {
    pub fn spawn(&self, future: impl Future<Output = ()> + 'static + Send) {
        let future = Box::pin(future);
        let task = Arc::new(Task {
            // future: Mutex::new(Some(future)),
            future: Mutex::new(future),
            sender: self.sender.clone(),
        });
        self.sender
            .send(task)
            .expect("receiver is disconnected or too many tasks queued");
    }
}

impl Executor {
    pub fn run(self) {
        while let Ok(task) = self.receiver.recv() {
            let mut future = task.future.lock().unwrap();
            let waker = unsafe {
                let _ = ManuallyDrop::new(task.clone());
                Waker::from_raw(RawWaker::new(
                    // Arc::into_raw(task.clone()).cast(),
                    Arc::as_ptr(&task).cast(),
                    &VTABLE,
                ))
            };
            let mut context = Context::from_waker(&waker);
            let _ = future.as_mut().poll(&mut context);
        }
    }
}


const VTABLE: RawWakerVTable = RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);

/// This function will be called when the RawWaker gets cloned, e.g. when the Waker in which the RawWaker is stored gets cloned.
///
/// The implementation of this function must retain all resources that are required for this additional instance of a RawWaker and associated task. Calling wake on the resulting RawWaker should result in a wakeup of the same task that would have been awoken by the original RawWaker.
unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
    let task = Arc::from_raw(ptr as *const Task);

    let _clone = ManuallyDrop::new(task.clone());
    let _task = ManuallyDrop::new(task);

    RawWaker::new(ptr, &VTABLE)
}

/// This function will be called when wake is called on the Waker. It must wake up the task associated with this RawWaker.
/// 
/// The implementation of this function must make sure to release any resources that are associated with this instance of a RawWaker and associated task.
unsafe fn wake(ptr: *const ()) {
    let task = Arc::from_raw(ptr as *const Task);
    task.wake();
}

/// This function will be called when wake_by_ref is called on the Waker. It must wake up the task associated with this RawWaker.
/// 
/// This function is similar to wake, but must not consume the provided data pointer.
unsafe fn wake_by_ref(ptr: *const ()) {
    let task = Arc::from_raw(ptr as *const Task);
    task.clone().wake();
    let _ = ManuallyDrop::new(task);
}

/// This function gets called when a Waker gets dropped.
/// 
/// The implementation of this function must make sure to release any resources that are associated with this instance of a RawWaker and associated task.
unsafe fn drop_waker(ptr: *const ()) {
    drop(Arc::from_raw(ptr as *const Task));
    // println!("task strong reference: {}", Arc::strong_count(&task));
}


pub fn spawn<T>(future: T) -> u64 
where
    T: Future + 'static + Send,
    T::Output: Send + 'static + Send
{
    0
}