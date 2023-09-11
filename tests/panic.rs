use std::future::Future;

fn test_panic<T: Future>(future: T) {
    use std::panic;
    use std::panic::AssertUnwindSafe;

    let output = panic::catch_unwind(AssertUnwindSafe(|| {
        struct Guard<'a, T> {
            data: &'a T,
        }
        impl<'a, T> Drop for Guard<'a, T> {
            fn drop(&mut self) {
                // paniced when poll
            }
        }

        let guard = Guard { data: &future };
        // let res = future.poll();
        std::mem::forget(guard);
        // res
    }));
}
