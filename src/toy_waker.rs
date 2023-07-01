use std::{task::{Waker, RawWaker, RawWakerVTable}, sync::Arc};


pub trait ArcWake: Send + Sync {
    fn wake(self: Arc<Self>);
}


pub fn waker_by_arc<T: ArcWake>(arc: &Arc<T>) -> Waker {
    unsafe {
        Waker::from_raw(RawWaker::new(
            Arc::into_raw(arc.clone()).cast(),
            &RawWakerVTable::new(clone_waker::<T>, wake::<T>, wake_by_ref::<T>, drop_waker::<T>),
        ))
    }
}

// const VTABLE: RawWakerVTable = RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);

/// This function will be called when the RawWaker gets cloned, e.g. when the Waker in which the RawWaker is stored gets cloned.
///
/// The implementation of this function must retain all resources that are required for this additional instance of a RawWaker and associated task. Calling wake on the resulting RawWaker should result in a wakeup of the same task that would have been awoken by the original RawWaker.
unsafe fn clone_waker<T: ArcWake>(ptr: *const ()) -> RawWaker {
    // increase refcount
    Arc::increment_strong_count(ptr as *const T);
    RawWaker::new(ptr, &RawWakerVTable::new(clone_waker::<T>, wake::<T>, wake_by_ref::<T>, drop_waker::<T>))
}

/// This function will be called when wake is called on the Waker. It must wake up the task associated with this RawWaker.
/// 
/// The implementation of this function must make sure to release any resources that are associated with this instance of a RawWaker and associated task.
unsafe fn wake<T: ArcWake>(ptr: *const ()) {
    let arc = Arc::from_raw(ptr as *const T);
    ArcWake::wake(arc);
}

/// This function will be called when wake_by_ref is called on the Waker. It must wake up the task associated with this RawWaker.
/// 
/// This function is similar to wake, but must not consume the provided data pointer.
unsafe fn wake_by_ref<T: ArcWake>(ptr: *const ()) {
    let arc = Arc::from_raw(ptr as *const T);
    ArcWake::wake(arc.clone());
    std::mem::forget(arc);
}

/// This function gets called when a Waker gets dropped.
/// 
/// The implementation of this function must make sure to release any resources that are associated with this instance of a RawWaker and associated task.
unsafe fn drop_waker<T: ArcWake>(ptr: *const ()) {
    // drop it
    let _ = Arc::from_raw(ptr as *const T);
}