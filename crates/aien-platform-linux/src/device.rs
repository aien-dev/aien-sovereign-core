//! Linux Compute Device implementation for Grace Blackwell GB10.
//! Binds to GB10 GPU with ATS coherency, or provides Linux host execution.

use crate::buffer::{LinuxMemoryKind, LinuxUnifiedBuffer, ResidencyPolicy};
use aien_platform::{
    BufferLayout, BufferRegion, ComputeDevice, ComputeWork, Fence, MemoryDevice, PlatformError,
};
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxCudaCapabilities {
    pub concurrent_managed_access: bool,
    pub pageable_memory_access: bool,
    pub uses_host_page_tables: bool,
    pub direct_managed_access_from_host: bool,
    pub host_native_atomics: bool,
}

impl LinuxCudaCapabilities {
    pub fn detect() -> Self {
        #[cfg(has_blackwell_cuda)]
        {
            let mut caps = Self {
                concurrent_managed_access: false,
                pageable_memory_access: false,
                uses_host_page_tables: false,
                direct_managed_access_from_host: false,
                host_native_atomics: false,
            };

            let get_attr = |attr: i32| -> i32 {
                let mut val = 0;
                let res = unsafe { cudaDeviceGetAttribute(&mut val, attr, 0) };
                if res == 0 {
                    val
                } else {
                    0
                }
            };

            caps.concurrent_managed_access = get_attr(89) != 0;
            caps.pageable_memory_access = get_attr(97) != 0;
            caps.uses_host_page_tables = get_attr(100) != 0;
            caps.direct_managed_access_from_host = get_attr(98) != 0;
            caps.host_native_atomics = get_attr(99) != 0;

            caps
        }
        #[cfg(not(has_blackwell_cuda))]
        {
            Self {
                concurrent_managed_access: false,
                pageable_memory_access: false,
                uses_host_page_tables: false,
                direct_managed_access_from_host: false,
                host_native_atomics: false,
            }
        }
    }

    pub fn is_ats_hardware_coherent(&self) -> bool {
        self.pageable_memory_access && self.uses_host_page_tables
    }
}

pub struct LinuxComputeDevice {
    device_id: u32,
    capabilities: LinuxCudaCapabilities,
    fence_counter: AtomicU64,
}

impl LinuxComputeDevice {
    pub fn new() -> Self {
        Self::with_device_id(0)
    }

    pub fn with_device_id(device_id: u32) -> Self {
        #[cfg(has_blackwell_cuda)]
        unsafe {
            let _ = cudaSetDevice(device_id as i32);
        }

        let capabilities = LinuxCudaCapabilities::detect();
        Self {
            device_id,
            capabilities,
            fence_counter: AtomicU64::new(1),
        }
    }

    pub fn device_id(&self) -> u32 {
        self.device_id
    }

    pub fn capabilities(&self) -> &LinuxCudaCapabilities {
        &self.capabilities
    }

    pub fn preferred_memory_kind(&self) -> LinuxMemoryKind {
        if self.capabilities.is_ats_hardware_coherent() {
            LinuxMemoryKind::AtsSystem
        } else if self.capabilities.pageable_memory_access {
            LinuxMemoryKind::HmmSystem
        } else {
            LinuxMemoryKind::CudaManagedFallback
        }
    }
}

impl Default for LinuxComputeDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputeDevice for LinuxComputeDevice {
    type Buffer = LinuxUnifiedBuffer;

    fn alloc(&self, layout: BufferLayout) -> Result<Self::Buffer, PlatformError> {
        let kind = self.preferred_memory_kind();
        LinuxUnifiedBuffer::allocate(layout, kind, ResidencyPolicy::FaultIn)
    }

    fn submit(&self, _work: ComputeWork<'_>) -> Result<Fence, PlatformError> {
        let id = self.fence_counter.fetch_add(1, Ordering::SeqCst);
        Ok(Fence(id))
    }

    fn synchronize(&self, _fence: Fence) -> Result<(), PlatformError> {
        #[cfg(has_blackwell_cuda)]
        unsafe {
            let res = cudaDeviceSynchronize();
            if res != 0 {
                return Err(PlatformError::HardwareFault(
                    "cudaDeviceSynchronize failed".into(),
                ));
            }
        }
        Ok(())
    }
}

impl MemoryDevice for LinuxComputeDevice {
    fn copy(&self, src: BufferRegion, dst: BufferRegion) -> Result<Fence, PlatformError> {
        if src.len != dst.len {
            return Err(PlatformError::InvalidLayout);
        }
        if src.len == 0 {
            let id = self.fence_counter.fetch_add(1, Ordering::SeqCst);
            return Ok(Fence(id));
        }

        #[cfg(has_blackwell_cuda)]
        {
            let res = unsafe {
                cudaMemcpyAsync(
                    dst.address.0 as *mut libc::c_void,
                    src.address.0 as *const libc::c_void,
                    src.len,
                    CUDA_MEMCPY_DEFAULT,
                    core::ptr::null_mut(),
                )
            };
            if res != 0 {
                return Err(PlatformError::HardwareFault(
                    "cudaMemcpyAsync failed".into(),
                ));
            }
        }
        #[cfg(not(has_blackwell_cuda))]
        {
            unsafe {
                core::ptr::copy(
                    src.address.0 as *const u8,
                    dst.address.0 as *mut u8,
                    src.len,
                );
            }
        }

        let id = self.fence_counter.fetch_add(1, Ordering::SeqCst);
        Ok(Fence(id))
    }

    fn zero(&self, dst: BufferRegion) -> Result<Fence, PlatformError> {
        if dst.len == 0 {
            let id = self.fence_counter.fetch_add(1, Ordering::SeqCst);
            return Ok(Fence(id));
        }

        #[cfg(has_blackwell_cuda)]
        {
            let res = unsafe {
                cudaMemsetAsync(
                    dst.address.0 as *mut libc::c_void,
                    0,
                    dst.len,
                    core::ptr::null_mut(),
                )
            };
            if res != 0 {
                return Err(PlatformError::HardwareFault(
                    "cudaMemsetAsync failed".into(),
                ));
            }
        }
        #[cfg(not(has_blackwell_cuda))]
        {
            unsafe {
                core::ptr::write_bytes(dst.address.0 as *mut u8, 0, dst.len);
            }
        }

        let id = self.fence_counter.fetch_add(1, Ordering::SeqCst);
        Ok(Fence(id))
    }
}

#[cfg(has_blackwell_cuda)]
const CUDA_MEMCPY_DEFAULT: i32 = 4;

#[cfg(has_blackwell_cuda)]
extern "C" {
    fn cudaDeviceGetAttribute(value: *mut i32, attr: i32, device: i32) -> i32;
    fn cudaSetDevice(device: i32) -> i32;
    fn cudaDeviceSynchronize() -> i32;
    fn cudaMemcpyAsync(
        dst: *mut libc::c_void,
        src: *const libc::c_void,
        count: usize,
        kind: i32,
        stream: *mut libc::c_void,
    ) -> i32;
    fn cudaMemsetAsync(
        dev_ptr: *mut libc::c_void,
        value: i32,
        count: usize,
        stream: *mut libc::c_void,
    ) -> i32;
}
