# toy-rumtime

Demonstrates rust async runtime

```rust
fn main() {
    let toy = Toy::new();

    for i in 1..=20 {
        toy.spawn(async move {
            let ret = FakeIO::new(Duration::from_secs(i)).await;
            println!("{:?}: {:?}", thread::current().id(), ret);
        })
    }

    toy.run(4); // 4 threads
}
```