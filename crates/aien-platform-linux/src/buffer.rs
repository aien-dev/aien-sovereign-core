//! Linux Unified Buffer implementation for Grace Blackwell GB10 and Linux host.
//! Allocates anonymous page-aligned mmap with ATS hardware coherency on GB10.

use aien_platform::{BufferLayout, DeviceAddress, PlatformError, UnifiedBuffer};
use core::mem::align_of;
use core::ptr::NonNull;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxMemoryKind {
    AtsSystem,
    HmmSystem,
    CudaManagedFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidencyPolicy {
    Pageable,
    FaultIn,
    Locked,
}

pub struct LinuxUnifiedBuffer {
    ptr: NonNull<u8>,
    mapping_base: NonNull<u8>,
    mapping_len: usize,
    len: usize,
    align: usize,
    kind: LinuxMemoryKind,
    device_address: DeviceAddress,
}

unsafe impl Send for LinuxUnifiedBuffer {}
unsafe impl Sync for LinuxUnifiedBuffer {}

impl LinuxUnifiedBuffer {
    pub fn allocate(
        layout: BufferLayout,
        kind: LinuxMemoryKind,
        policy: ResidencyPolicy,
    ) -> Result<Self, PlatformError> {
        if layout.size == 0 {
            return Err(PlatformError::InvalidLayout);
        }

        let align = layout.align.max(align_of::<usize>()).max(64);
        let mapping_len = layout
            .size
            .checked_add(align)
            .ok_or(PlatformError::OutOfMemory)?;

        let (mapping_base_ptr, actual_kind) = match kind {
            LinuxMemoryKind::AtsSystem | LinuxMemoryKind::HmmSystem => {
                let p = unsafe {
                    libc::mmap(
                        core::ptr::null_mut(),
                        mapping_len,
                        libc::PROT_READ | libc::PROT_WRITE,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                        -1,
                        0,
                    )
                };
                if p == libc::MAP_FAILED || p.is_null() {
                    return Err(PlatformError::OutOfMemory);
                }
                (p as *mut u8, kind)
            }
            LinuxMemoryKind::CudaManagedFallback => {
                #[cfg(has_blackwell_cuda)]
                {
                    let mut p: *mut libc::c_void = core::ptr::null_mut();
                    let res = unsafe { cudaMallocManaged(&mut p, mapping_len, 1) };
                    if res == 0 && !p.is_null() {
                        (p as *mut u8, LinuxMemoryKind::CudaManagedFallback)
                    } else {
                        let fallback_ptr = unsafe {
                            libc::mmap(
                                core::ptr::null_mut(),
                                mapping_len,
                                libc::PROT_READ | libc::PROT_WRITE,
                                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                                -1,
                                0,
                            )
                        };
                        if fallback_ptr == libc::MAP_FAILED || fallback_ptr.is_null() {
                            return Err(PlatformError::OutOfMemory);
                        }
                        (fallback_ptr as *mut u8, LinuxMemoryKind::AtsSystem)
                    }
                }
                #[cfg(not(has_blackwell_cuda))]
                {
                    let p = unsafe {
                        libc::mmap(
                            core::ptr::null_mut(),
                            mapping_len,
                            libc::PROT_READ | libc::PROT_WRITE,
                            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                            -1,
                            0,
                        )
                    };
                    if p == libc::MAP_FAILED || p.is_null() {
                        return Err(PlatformError::OutOfMemory);
                    }
                    (p as *mut u8, LinuxMemoryKind::AtsSystem)
                }
            }
        };

        let mapping_base = NonNull::new(mapping_base_ptr).ok_or(PlatformError::OutOfMemory)?;
        let base_addr = mapping_base_ptr as usize;
        let aligned_addr = (base_addr + align - 1) & !(align - 1);
        let ptr = NonNull::new(aligned_addr as *mut u8).ok_or(PlatformError::OutOfMemory)?;

        if policy == ResidencyPolicy::FaultIn {
            let page_size = 4096;
            for offset in (0..layout.size).step_by(page_size) {
                unsafe {
                    core::ptr::write_volatile(ptr.as_ptr().add(offset), 0);
                }
            }
        }

        let device_address = DeviceAddress(aligned_addr as u64);

        Ok(Self {
            ptr,
            mapping_base,
            mapping_len,
            len: layout.size,
            align,
            kind: actual_kind,
            device_address,
        })
    }

    pub fn kind(&self) -> LinuxMemoryKind {
        self.kind
    }

    pub fn alignment(&self) -> usize {
        self.align
    }
}

impl UnifiedBuffer for LinuxUnifiedBuffer {
    fn len(&self) -> usize {
        self.len
    }

    fn device_address(&self) -> DeviceAddress {
        self.device_address
    }

    fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }
}

impl Drop for LinuxUnifiedBuffer {
    fn drop(&mut self) {
        match self.kind {
            LinuxMemoryKind::AtsSystem | LinuxMemoryKind::HmmSystem => unsafe {
                libc::munmap(
                    self.mapping_base.as_ptr() as *mut libc::c_void,
                    self.mapping_len,
                );
            },
            LinuxMemoryKind::CudaManagedFallback => {
                #[cfg(has_blackwell_cuda)]
                unsafe {
                    cudaFree(self.mapping_base.as_ptr() as *mut libc::c_void);
                }
                #[cfg(not(has_blackwell_cuda))]
                unsafe {
                    libc::munmap(
                        self.mapping_base.as_ptr() as *mut libc::c_void,
                        self.mapping_len,
                    );
                }
            }
        }
    }
}

#[cfg(has_blackwell_cuda)]
extern "C" {
    fn cudaMallocManaged(dev_ptr: *mut *mut libc::c_void, size: usize, flags: u32) -> i32;
    fn cudaFree(dev_ptr: *mut libc::c_void) -> i32;
}
