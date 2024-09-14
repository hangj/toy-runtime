mod fake_io;
mod toy;

use toy::Toy;

use fake_io::FakeIO;
use std::thread;
use std::time::Duration;

fn main() {
    let toy = Toy::new();

    for i in 1..=20 {
        toy.spawn(async move {
            let ret = FakeIO::new(Duration::from_secs(i)).await;
            println!("{:?}: {:?}", thread::current().id(), ret);
        });
    }

    toy.run(4);
}
