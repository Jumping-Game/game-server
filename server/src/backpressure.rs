use std::collections::VecDeque;

#[derive(Debug)]
pub struct BoundedQueue<T> {
    capacity: usize,
    queue: VecDeque<T>,
    dropped: u32,
}

impl<T> BoundedQueue<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            queue: VecDeque::new(),
            dropped: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.queue.len() >= self.capacity {
            self.queue.pop_front();
            self.dropped += 1;
        }
        self.queue.push_back(item);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.queue.pop_front()
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_oldest_when_full() {
        let mut queue = BoundedQueue::new(2);
        queue.push(1);
        queue.push(2);
        queue.push(3);
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.dropped(), 1);
    }
}
