use crate::compute::ComputeDevice;
use crate::queue::WorkQueue;

/// Zero-cost, statically dispatched inference runtime.
/// Avoids dynamic vtables across the platform boundary for maximum inlining and zero context switching.
pub struct Runtime<D, Q>
where
    D: ComputeDevice,
    Q: WorkQueue,
{
    pub device: D,
    pub queue: Q,
}

impl<D, Q> Runtime<D, Q>
where
    D: ComputeDevice,
    Q: WorkQueue,
{
    pub const fn new(device: D, queue: Q) -> Self {
        Self { device, queue }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::*;
    use crate::queue::*;
    use crate::PlatformError;
    use alloc::vec;
    use alloc::vec::Vec;

    struct MockBuffer(Vec<u8>);
    impl UnifiedBuffer for MockBuffer {
        fn len(&self) -> usize {
            self.0.len()
        }
        fn device_address(&self) -> DeviceAddress {
            DeviceAddress(self.0.as_ptr() as u64)
        }
        fn as_ptr(&self) -> *const u8 {
            self.0.as_ptr()
        }
        fn as_mut_ptr(&mut self) -> *mut u8 {
            self.0.as_mut_ptr()
        }
    }

    struct MockDevice;
    impl ComputeDevice for MockDevice {
        type Buffer = MockBuffer;

        fn alloc(&self, layout: BufferLayout) -> Result<Self::Buffer, PlatformError> {
            Ok(MockBuffer(vec![0u8; layout.size]))
        }

        fn submit(&self, _work: ComputeWork<'_>) -> Result<Fence, PlatformError> {
            Ok(Fence(1))
        }

        fn synchronize(&self, _fence: Fence) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    struct MockQueue(spin::Mutex<Vec<InferenceWork>>);
    impl WorkQueue for MockQueue {
        fn push(&self, work: InferenceWork) -> Result<(), QueueError> {
            self.0.lock().push(work);
            Ok(())
        }
        fn pop(&self) -> Option<InferenceWork> {
            self.0.lock().pop()
        }
        fn len(&self) -> usize {
            self.0.lock().len()
        }
    }

    #[test]
    fn test_statically_dispatched_runtime() {
        let device = MockDevice;
        let queue = MockQueue(spin::Mutex::new(Vec::new()));
        let runtime = Runtime::new(device, queue);

        let buf = runtime.device.alloc(BufferLayout::new(1024, 64)).unwrap();
        assert_eq!(buf.len(), 1024);

        let work = InferenceWork {
            sequence: 1,
            model: ModelHandle(10),
            kv: KvHandle(100),
            priority: Priority::Interactive,
            deadline: Some(500),
            branch_parent: None,
            next_token_budget: 32,
        };

        runtime.queue.push(work.clone()).unwrap();
        assert_eq!(runtime.queue.len(), 1);
        let popped = runtime.queue.pop().unwrap();
        assert_eq!(popped, work);
    }
}
