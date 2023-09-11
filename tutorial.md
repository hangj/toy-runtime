# async-await Rust: 200 多行代码实现一个极简 runtime

> What I cannot create, I do not understand 

Rust 中的 runtime 到底是咋回事, 为了彻底搞懂它, 我在尽量不借助第三方 crate 的情况下实现了一个玩具 runtime, 之所以说是玩具，因为它没有复杂的调度算法(只有一个全局 task queue)  

代码除了 mpmc(multi-producer, multi-consumer) 使用第三方 crate [crossbeam](https://crates.io/crates/crossbeam) 之外, 其余代码一律手撸


可以这么玩

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

其中 FakeIO 也是足够单纯

```rust
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
```

# 数据结构

数据结构就下面几个

```rust
struct Task {
    raw: RawTask,
}

unsafe impl Send for Task {}
unsafe impl Sync for Task {}

struct RawTask {
    ptr: NonNull<Header>, // pointer to Cell<T> where T: Future
}

struct Header {
    // todo: maybe replace the Mutex<State> with AtomicUsize
    state: Mutex<State>,
    vtable: &'static Vtable,
    sender: crossbeam::channel::Sender<Task>,
}

#[derive(Default)]
struct State {
    running: bool,
    notified: bool,
    completed: bool,
}

/// #[repr(C)] make sure `*mut Cell<T>` can cast to valid `*mut Header`, and backwards. 
/// In the default situation, the data layout may not be the same as the order in which the fields are specified in the declaration of the type
/// 默认情况下 Rust 的数据布局不一定会按照 field 的声明顺序排列
/// [The Default Representation](https://doc.rust-lang.org/reference/type-layout.html?#the-default-representation)
///
/// [playground link](https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=39ac84782d121970598b91201b168f82)
/// 
/// you can easilly view the data layout with this crate https://github.com/hangj/layout-rs
#[repr(C)]
struct Cell<T: Future> {
    header: Header,
    future: T,
    output: Option<T::Output>,
}

struct Vtable {
    poll_task: unsafe fn(NonNull<Header>),
    clone_task: unsafe fn(NonNull<Header>) -> NonNull<Header>,
    drop_task: unsafe fn(NonNull<Header>),
}
```

其中值得注意的是: 
- `RawTask` 内的 `ptr` 实际上指向的是 `NonNull<Cell<T: Future>>`
- `Cell<T: Future>` 被标记了 `#repr(C)`, 原因已在注释中说明
- `vtable` 的设计参考了 [Waker](https://doc.rust-lang.org/src/core/task/wake.rs.html#15) 中的 vtable, 相当于利用泛型函数保存了类型信息, 便于后面从裸指针恢复到原始类型

点击「阅读原文」直达 [toy-runtime](https://github.com/hangj/toy-runtime) 仓库

Have fun!
