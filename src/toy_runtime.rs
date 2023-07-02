use std::{
    future::Future,
    pin::Pin,
    ptr::NonNull,
    sync::{mpsc::channel, Arc, Mutex},
    task::Context,
};

use crate::{
    handle::Handle,
    msg_queue::{self, Mq},
    toy_waker::{self, ArcWake},
};

pub struct Task {
    raw_task: NonNull<()>,
    vtable: &'static Vtable,
    // sender: SyncSender<Arc<Mutex<Task>>>,
    sender: msg_queue::Sender,
    output_sender: NonNull<()>,
}

unsafe impl Send for Task {}

pub struct Vtable {
    /// Polls the future.
    pub poll_future: unsafe fn(NonNull<()>, cx: &mut Context<'_>, NonNull<()>),
    pub drop_future: unsafe fn(NonNull<()>),
    pub drop_output_sender: unsafe fn(NonNull<()>),
}

impl Drop for Task {
    fn drop(&mut self) {
        unsafe {
            (self.vtable.drop_future)(self.raw_task);
            (self.vtable.drop_output_sender)(self.output_sender);
        }
    }
}

impl ArcWake for Mutex<Task> {
    fn wake(self: Arc<Self>) {
        let task = self.lock().unwrap();
        // task.sender.send(self.clone()).expect("receiver is disconnected or too many tasks queued");
        task.sender
            .send(unsafe { NonNull::new_unchecked(Arc::into_raw(self.clone()).cast_mut()) }.cast());
    }
}

impl Task {
    // fn from_future<T>(future: T, sender: SyncSender<Arc<Mutex<Task>>>) -> Self
    fn new<T>(future: T, sender: msg_queue::Sender, output_sender: NonNull<()>) -> Self
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let raw_task = NonNull::new(Box::into_raw(Box::new(future)))
            .unwrap()
            .cast();
        let vtable = raw_vtable::<T>();
        Self {
            raw_task,
            vtable,
            sender,
            output_sender,
        }
    }

    fn poll(this: Arc<Mutex<Self>>, cx: &mut Context<'_>) {
        let task = this.lock().unwrap();
        unsafe {
            (task.vtable.poll_future)(task.raw_task, cx, task.output_sender);
        }
    }
}

fn raw_vtable<T>() -> &'static Vtable
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    &Vtable {
        poll_future: raw_poll_future::<T>,
        drop_future: raw_drop_future::<T>,
        drop_output_sender: raw_drop_output_sender::<T::Output>,
    }
}

unsafe fn raw_poll_future<T>(ptr_future: NonNull<()>, cx: &mut Context<'_>, ptr_output: NonNull<()>)
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    let p: NonNull<std::sync::mpsc::Sender<_>> =
        ptr_output.cast::<std::sync::mpsc::Sender<T::Output>>();
    let sender = Box::leak(Box::from_raw(p.as_ptr()));

    let mut p = ptr_future.cast::<T>();
    let future = Pin::new_unchecked(p.as_mut());
    match future.poll(cx) {
        std::task::Poll::Ready(v) => {
            let _ = sender.send(v);
        }
        std::task::Poll::Pending => {}
    }
}

unsafe fn raw_drop_future<T>(ptr: NonNull<()>)
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    let p = ptr.cast::<T>();
    let _boxed = Box::from_raw(p.as_ptr());
}

unsafe fn raw_drop_output_sender<Output>(ptr: NonNull<()>)
where
    Output: 'static + Send,
{
    let p = ptr.cast::<std::sync::mpsc::Sender<Output>>();
    let _boxed = Box::from_raw(p.as_ptr());
}

#[derive(Clone)]
pub struct Toy {
    sender: msg_queue::Sender,
    receiver: msg_queue::Receiver,
}

impl Toy {
    pub fn new() -> Self {
        let (sender, receiver) = Mq::new().split();
        Self { sender, receiver }
    }

    pub fn spawn<T>(&self, future: T) -> Handle<T::Output>
    where
        T: Future + 'static + Send,
        T::Output: Send + 'static + Send,
    {
        let (tx, rx) = channel();
        let raw_tx = Box::into_raw(Box::new(tx));
        let non_null = unsafe { NonNull::new_unchecked(raw_tx) }.cast();

        let task = Arc::new(Mutex::new(Task::new(future, self.sender.clone(), non_null)));

        self.sender
            .send(unsafe { NonNull::new_unchecked(Arc::into_raw(task).cast_mut()) }.cast());

        Handle::from_receiver(rx)
    }

    pub fn block_on<T: 'static + Send>(self, handles: Vec<Handle<T>>) {
        let receiver = self.receiver;

        let mut threads = Vec::new();

        for _ in 0..4 {
            let receiver = receiver.clone();

            let th = std::thread::spawn(move || {
                let receiver = receiver;
                loop {
                    let ptr = receiver.recv();
                    let task = unsafe { Arc::from_raw(ptr.cast().as_ptr()) };
                    let waker = toy_waker::waker_by_arc(&task);
                    let mut context = Context::from_waker(&waker);

                    Task::poll(task, &mut context);
                }
                // println!("executor finished.");
            });

            threads.push(th);
        }

        for handle in handles.iter() {
            let _ = handle.receiver.recv();
        }

        // for th in threads {
        //     let _ = th.join();
        // }
    }
}
