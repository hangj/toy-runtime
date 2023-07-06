use std::{
    future::Future,
    sync::{Arc, Mutex},
    task::Poll,
    thread,
    time::Duration,
};

#[derive(Default)]
pub struct FakeIO {
    shared_state: Arc<Mutex<SharedState>>,
    duration: Duration,
}

#[derive(Default)]
struct SharedState {
    completed: bool,
}

impl Future for FakeIO {
    type Output = Duration;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let state = self.shared_state.lock().unwrap();
        if state.completed {
            return Poll::Ready(self.duration);
        }

        let state = self.shared_state.clone();
        let waker = cx.waker().clone();
        let duration = self.duration;

        thread::spawn(move || {
            thread::sleep(duration);

            {
                let mut state = state.lock().unwrap();
                state.completed = true;
            }

            waker.wake();
        });

        Poll::Pending
    }
}

impl FakeIO {
    pub fn new(duration: Duration) -> Self {
        let shared_state = Arc::new(Mutex::new(SharedState::default()));
        Self {
            shared_state,
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
