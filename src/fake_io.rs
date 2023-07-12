use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Poll,
    thread,
    time::Duration,
};

pub struct FakeIO {
    finished: Arc<AtomicBool>,
    duration: Duration,
}

impl Future for FakeIO {
    type Output = Duration;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.finished.load(Ordering::Acquire) {
            return Poll::Ready(self.duration);
        }

        let finished = self.finished.clone();
        let waker = cx.waker().clone();
        let duration = self.duration;

        thread::spawn(move || {
            thread::sleep(duration);

            finished.store(true, Ordering::Release);

            waker.wake();
        });

        Poll::Pending
    }
}

impl FakeIO {
    pub fn new(duration: Duration) -> Self {
        Self {
            finished: Arc::new(false.into()),
            duration,
        }
    }
}

#[cfg(test)]
mod test_pin {
    use std::{marker::PhantomPinned, pin::Pin};

    #[derive(Debug)]
    struct Unmovable {
        pub data: i32,
        // slice: NonNull<String>,
        _pin: PhantomPinned,
    }

    impl Unmovable {
        fn new(data: i32) -> Self {
            Self {
                data,
                _pin: PhantomPinned,
            }
        }

        pub fn change(self: Pin<&mut Self>, data: i32) {
            unsafe {
                self.get_unchecked_mut().data = data;
            }
        }
    }

    #[test]
    fn test() {
        let val = Unmovable::new(42);
        let mut pin = Box::pin(val);
        pin.as_mut().change(43);

        println!("pin: {:#?}", pin);
    }
}
