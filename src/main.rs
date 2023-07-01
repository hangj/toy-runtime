mod fake_io;
mod msg_queue;
mod toy_runtime;
mod toy_waker;

use std::time::Duration;

use toy_runtime::Toy;

fn main() {
    let toy = Toy::new();

    for i in 1..5 {
        toy.spawn(async move {
            let ret = fake_io::FakeIO::new(Duration::from_secs(i)).await;
            println!("{:?}", ret);
        });
    }

    toy.run();
}
