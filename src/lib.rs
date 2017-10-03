pub mod nothing;

pub trait Queue<T> {
    fn new() -> Self;
    fn push(&self, T);
    fn pop(&self) -> Option<T>;
    fn is_empty(&self) -> bool;
}

impl<T> Queue<T> for nothing::Queue<T> {
    fn new() -> Self {
        nothing::Queue::new()
    }
    fn push(&self, val: T) {
        nothing::Queue::push(self, val);
    }
    fn pop(&self) -> Option<T> {
        nothing::Queue::pop(self)
    }
    fn is_empty(&self) -> bool {
        nothing::Queue::is_empty(self)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const N_THREADS: usize = 4;

    // TODO(martin): rewrite this, so we can write `queue_test!(nothing)`
    macro_rules! queue_test {($Q:ty) => {
        #[test]
        fn correct_queue() {
            println!("type: {}", stringify!($Q));
            let q: $Q = Queue::new();
            q.push(123);
            assert!(!q.is_empty());
            assert_eq!(q.pop(), Some(123));
            assert!(q.is_empty());
            assert!(false);
        }
    }}

    queue_test!(nothing::Queue<u32>);
}
