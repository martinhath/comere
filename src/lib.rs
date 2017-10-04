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

pub trait List<T> {
    fn new() -> Self;
    fn insert(&self, T);
    // fn remove(&self) -> Option<T>;
    fn is_empty(&self) -> bool;
}

impl<T> List<T> for nothing::list::List<T> {
    fn new() -> Self {
        nothing::list::List::new()
    }
    fn insert(&self, val: T) {
        nothing::list::List::insert(self, val);
    }
    // fn remove(&self) -> Option<T> {
    //     nothing::list::List::remove(self)
    // }
    fn is_empty(&self) -> bool {
        nothing::list::List::is_empty(self)
    }
}


#[cfg(test)]
mod test {
    use super::*;

    const N_THREADS: usize = 4;

    macro_rules! correctness_queue {($Q:ident) => {
        $Q.push(123);
        assert!(!$Q.is_empty());
        assert_eq!($Q.pop(), Some(123));
        assert!($Q.is_empty());
        for i in 0..200 {
            $Q.push(i);
        }
        assert!(!$Q.is_empty());
        for i in 0..200 {
            assert_eq!($Q.pop(), Some(i));
        }
        assert!($Q.is_empty());
    }}

    #[test]
    fn correct_queue_nothing() {
        let q: nothing::queue::Queue<u32> = Queue::new();
        correctness_queue!(q);
    }


    macro_rules! correctness_list {($L:ident) => {
        assert!($L.is_empty());
        $L.insert(1);
        assert!(!$L.is_empty());
        assert_eq!($L.remove_front(), Some(1));
        assert!($L.is_empty());
        for i in 0..200 {
            $L.insert(i);
        }
        assert!(!$L.is_empty());
        for i in (0..200).rev() {
            assert_eq!($L.remove_front(), Some(i));
        }

    }}

    #[test]
    fn correct_list_nothing() {
        let l: nothing::list::List<u32> = List::new();
        correctness_list!(l);
    }

}
