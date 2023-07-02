mod fake_io;
mod handle;
mod msg_queue;
mod toy_runtime;
mod toy_waker;

use std::time::Duration;

use toy_runtime::Toy;

fn main() {
    let toy = Toy::new();

    let handles: Vec<_> = (1..=5)
        .map(|i| {
            toy.spawn(async move {
                let ret = fake_io::FakeIO::new(Duration::from_secs(i)).await;
                println!("{:?}", ret);
            })
        })
        .collect();

    toy.block_on(handles);
}
