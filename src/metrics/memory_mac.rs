#[allow(deprecated)]
use libc::{c_int, c_void, mach_host_self, mach_port_t, sysctlbyname};
use std::convert::TryInto;
use std::ffi::CString;
use std::mem;

const HOST_VM_INFO64: c_int = 4;
const HOST_VM_INFO64_COUNT: u32 =
    (mem::size_of::<vm_statistics64>() / mem::size_of::<c_int>()) as u32;
const KERN_SUCCESS: c_int = 0;

#[repr(C)]
struct vm_statistics64 {
    free_count: u32,
    active_count: u32,
    inactive_count: u32,
    wire_count: u32,
    zero_fill_count: u64,
    reactivations: u64,
    pageins: u64,
    pageouts: u64,
    faults: u64,
    cow_faults: u64,
    lookups: u64,
    hits: u64,
    purges: u64,
    purgeable_count: u32,
    speculative_count: u32,
    decompressions: u64,
    compressions: u64,
    swapins: u64,
    swapouts: u64,
    compressor_page_count: u32,
    throttled_count: u32,
    external_page_count: u32,
    internal_page_count: u32,
    total_uncompressed_pages_in_compressor: u64,
}

extern "C" {
    fn host_statistics64(
        host_priv: mach_port_t,
        flavor: c_int,
        host_info_out: *mut c_int,
        host_info_outCnt: *mut u32,
    ) -> c_int;

    fn host_page_size(host: mach_port_t, out_page_size: *mut usize) -> c_int;
}

fn get_vm_page_pageable_internal_count() -> Option<u64> {
    let mut buf: Vec<u8> = vec![0; 8];
    let c = CString::new("vm.page_pageable_internal_count").ok()?;
    let mut len: usize = 8;
    unsafe {
        if sysctlbyname(
            c.as_ptr(),
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }
        Some(u64::from_ne_bytes(buf[..8].try_into().ok()?))
    }
}

pub fn get_macos_memory_used() -> Option<u64> {
    unsafe {
        #[allow(deprecated)]
        let host_port = mach_host_self();
        let mut vm_stat: vm_statistics64 = mem::zeroed();
        let mut count = HOST_VM_INFO64_COUNT;

        let ret = host_statistics64(
            host_port,
            HOST_VM_INFO64,
            &mut vm_stat as *mut _ as *mut c_int,
            &mut count,
        );

        if ret != KERN_SUCCESS {
            return None;
        }

        // Get page size using host_page_size, fallback to sysconf
        let mut page_size: usize = 0;
        let res = host_page_size(host_port, &mut page_size);
        if res != KERN_SUCCESS {
            page_size = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        }

        // Get pageable internal count for accurate app memory
        let pageable_internal = get_vm_page_pageable_internal_count()?;

        let app_mem = pageable_internal.saturating_sub(vm_stat.purgeable_count as u64);
        let wired_mem = vm_stat.wire_count as u64;
        let compressed_mem = vm_stat.compressor_page_count as u64;

        Some(
            app_mem
                .saturating_add(compressed_mem)
                .saturating_add(wired_mem)
                .saturating_mul(page_size as u64),
        )
    }
}
