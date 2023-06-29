mod fake_io;
mod mini_runtime;

use std::time::Duration;


mod context;
mod thread_id;

fn main() {
    let (executor, spawner) = mini_runtime::new_executor_and_spawner();

    for i in 1..5 {
        spawner.spawn(async move {
            let ret = fake_io::FakeIO::new(Duration::from_secs(i)).await;
            println!("{:?}", ret);
        });
    }

    // Drop the spawner so that our executor knows it is finished and won't
    // receive more incoming tasks to run.
    drop(spawner);

    executor.run();
}
