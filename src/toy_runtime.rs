use std::{
    future::Future,
    pin::Pin,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Mutex,
    },
    task::Context, ptr::NonNull,
};

use crate::{toy_waker::{ArcWake, self}, msg_queue::{Mq, self}};

pub struct Task {
    raw_task: NonNull<()>,
    vtable: &'static Vtable,
    // sender: SyncSender<Arc<Mutex<Task>>>,
    sender: msg_queue::Sender,
}

unsafe impl Send for Task {}

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

impl ArcWake for Mutex<Task> {
    fn wake(self: Arc<Self>) {
        let task = self.lock().unwrap();
        // task.sender.send(self.clone()).expect("receiver is disconnected or too many tasks queued");
        task.sender.send(unsafe{NonNull::new_unchecked(Arc::into_raw(self.clone()).cast_mut())}.cast());
        
    }
}

impl Task {
    // fn from_future<T>(future: T, sender: SyncSender<Arc<Mutex<Task>>>) -> Self
    fn from_future<T>(future: T, sender: msg_queue::Sender) -> Self
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let raw_task = NonNull::new(Box::into_raw(Box::new(future))).unwrap().cast();
        let vtable = raw_vtable::<T>();
        Self {
            raw_task,
            vtable,
            sender,
        }
    }

    fn poll(this: Arc<Mutex<Self>>, cx: &mut Context<'_>) {
        let task = this.lock().unwrap();
        unsafe {(task.vtable.poll)(task.raw_task, cx);}
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
    let mut p = ptr.cast::<T>();
    let future = Pin::new_unchecked(p.as_mut());
    let _ = future.poll(cx);
}

unsafe fn raw_drop<T>(ptr: NonNull<()>)
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    let p = ptr.cast::<T>();
    let _boxed = Box::from_raw(p.as_ptr());
}

pub struct Toy {
    sender: msg_queue::Sender,
    receiver: msg_queue::Receiver,
}

impl Toy {
    pub fn new() -> Self {
        let (sender, receiver) = Mq::new().split();
        Self {
            sender,
            receiver,
        }
    }

    pub fn spawn<T>(&self, future: T) -> u64
    where
        T: Future + 'static + Send,
        T::Output: Send + 'static + Send
    {
        let task = Arc::new(Mutex::new(Task::from_future(future, self.sender.clone() )));

        self.sender
            .send(unsafe{NonNull::new_unchecked(Arc::into_raw(task).cast_mut())}.cast());
        0
    }

    pub fn run(self) {
        let receiver = self.receiver;

        let mut threads = Vec::new();

        for _ in 0..4 {
            let receiver = receiver.clone();

            let th = std::thread::spawn(move || {
                let receiver = receiver;
                loop {
                    let ptr = receiver.recv();
                    let task = unsafe{Arc::from_raw(ptr.cast().as_ptr())};
                    let waker = toy_waker::waker_by_arc(&task);
                    let mut context = Context::from_waker(&waker);
        
                    Task::poll(task, &mut context);
                }
                println!("executor finished.");
            });

            threads.push(th);
        }

        for th in threads {
            let _ = th.join();
        }
    }
}