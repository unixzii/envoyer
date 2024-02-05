use std::collections::VecDeque;

use tokio::sync::RwLock;

pub struct StdoutBuffer {
    buf: RwLock<VecDeque<u8>>,
}

impl StdoutBuffer {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: RwLock::new(VecDeque::with_capacity(capacity)),
        }
    }

    pub async fn write(&self, data: &[u8]) {
        let mut buf = self.buf.write().await;

        let used_src_len = data.len().min(buf.capacity());

        let new_len = buf.len() + used_src_len;
        if new_len <= buf.capacity() {
            buf.extend(data);
            return;
        }

        // No enough room for the data, evict the old data first.
        let evict_len = (new_len - buf.capacity()).min(buf.len());
        buf.drain(0..evict_len);

        buf.extend(&data[(data.len() - used_src_len)..]);
    }

    pub async fn get_buffer(&self) -> Vec<u8> {
        let buf = self.buf.read().await;

        let mut vec = Vec::with_capacity(buf.len());
        let (part_1, part_2) = buf.as_slices();
        vec.extend(part_1);
        vec.extend(part_2);

        vec
    }
}

impl Default for StdoutBuffer {
    fn default() -> Self {
        Self::with_capacity(64 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_small() {
        let buf = StdoutBuffer::with_capacity(128);
        buf.write(&[1, 2, 3]).await;
        assert_eq!(buf.get_buffer().await, vec![1, 2, 3]);
        buf.write(&[4, 5]).await;
        assert_eq!(buf.get_buffer().await, vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn test_write_large() {
        let buf = StdoutBuffer::with_capacity(4);
        buf.write(&[1, 2, 3]).await;
        assert_eq!(buf.get_buffer().await, vec![1, 2, 3]);
        buf.write(&[4, 5]).await;
        assert_eq!(buf.get_buffer().await, vec![2, 3, 4, 5]);
        buf.write(&[6, 7, 8, 9, 10, 11, 12]).await;
        assert_eq!(buf.get_buffer().await, vec![9, 10, 11, 12]);
    }
}
