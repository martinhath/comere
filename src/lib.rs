pub mod nothing;

pub trait Queue<T> {
    fn new() -> Self;
    fn push(&self, T);
    fn pop(&self) -> Option<T>;
    fn is_empty(&self) -> bool;
}

impl<T> Queue<T> for nothing::queue::Queue<T> {
    fn new() -> Self {
        nothing::queue::Queue::new()
    }
    fn push(&self, val: T) {
        nothing::queue::Queue::push(self, val);
    }
    fn pop(&self) -> Option<T> {
        nothing::queue::Queue::pop(self)
    }
    fn is_empty(&self) -> bool {
        nothing::queue::Queue::is_empty(self)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const N_THREADS: usize = 4;

    macro_rules! correctness {($Q:ident) => {
        $Q.push(123);
        assert!(!$Q.is_empty());
        assert_eq!($Q.pop(), Some(123));
        assert!($Q.is_empty());
    }}

    #[test]
    fn correct_queue_nothing() {
        let q: nothing::queue::Queue<u32> = Queue::new();
        correctness!(q);
    }
}
