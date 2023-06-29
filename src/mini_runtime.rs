use std::{
    future::{Future, IntoFuture},
    pin::Pin,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Mutex,
    },
    task::{Context, RawWaker, RawWakerVTable, Waker}, mem::ManuallyDrop, ptr::NonNull, os::macos::raw,
};

use crate::context::get_sender;

pub struct Executor {
    receiver: Receiver<Arc<Task>>,
}

pub struct Spawner {
    sender: SyncSender<Arc<Task>>,
}

pub struct Task {
    raw_task: NonNull<()>,
    vtable: &'static Vtable,
    sender: SyncSender<Arc<Task>>,
}

pub(super) struct Vtable {
    /// Polls the future.
    pub(super) poll: unsafe fn(NonNull<()>, cx: &mut Context<'_>),
    pub(super) drop: unsafe fn(NonNull<()>),
}

impl Drop for Task {
    fn drop(&mut self) {
        unsafe {(self.vtable.drop)(self.raw_task);}
    }
}

impl Task {
    fn from_raw<T>(raw_task: T, sender: SyncSender<Arc<Task>>) -> Self
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let raw_task = NonNull::new(Box::into_raw(Box::new(raw_task))).unwrap().cast();
        let vtable = raw_vtable::<T>();
        Self {
            raw_task,
            vtable,
            sender,
        }
    }

    fn wake(self: Arc<Self>) {
        self.sender.send(self.clone()).expect("receiver is disconnected or too many tasks queued");
    }

    fn poll(self: Arc<Self>, cx: &mut Context<'_>) {
        unsafe {(self.vtable.poll)(self.raw_task, cx);}
    }
}

fn raw_vtable<T>() -> &'static Vtable 
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    &Vtable {
        poll: raw_poll::<T>,
        drop: raw_drop::<T>,
    }
}

unsafe fn raw_poll<T>(ptr: NonNull<()>, cx: &mut Context<'_>)
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    let p = ptr.cast::<T>();
    let mut boxed = Box::from_raw(p.as_ptr());
    let mut future = Pin::new_unchecked(boxed.as_mut());
    let _ = future.as_mut().poll(cx).is_pending();

    let _ = ManuallyDrop::new(boxed);
}

unsafe fn raw_drop<T>(ptr: NonNull<()>)
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    let p = ptr.cast::<T>();
    let _boxed = Box::from_raw(p.as_ptr());
}

pub fn new_executor_and_spawner() -> (Executor, Spawner) {
    const MAX_QUEUED_TASKS: usize = 10_000;
    let (sender, receiver) = sync_channel(MAX_QUEUED_TASKS);
    (Executor { receiver }, Spawner { sender })
}

impl Spawner {
    pub fn spawn<T>(&self, future: T)
    where
        T: 'static + Future + Send,
        T::Output: 'static + Send,
    {
        // let future = Box::pin(future);
        let task = Arc::new(Task::from_raw(future, self.sender.clone() ));

        self.sender
            .send(task)
            .expect("receiver is disconnected or too many tasks queued");
    }
}

impl Executor {
    pub fn run(self) {
        while let Ok(task) = self.receiver.recv() {
            // let mut future = task.future.lock().unwrap();
            let waker = unsafe {
                let _ = ManuallyDrop::new(task.clone());
                Waker::from_raw(RawWaker::new(
                    // Arc::into_raw(task.clone()).cast(),
                    Arc::as_ptr(&task).cast(),
                    &VTABLE,
                ))
            };
            let mut context = Context::from_waker(&waker);
            task.poll(&mut context);
            // let _ = future.as_mut().poll(&mut context);
        }
        println!("executor finished.");
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