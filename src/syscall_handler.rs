use std::ffi::c_void;

use melior::ExecutionEngine;

#[derive(Debug, Default)]
pub struct SyscallContext {
    pub memory: Vec<u8>,
    result: Option<(usize, usize)>,
}

// Syscall implementations
impl SyscallContext {
    fn write_result(&mut self, offset: usize, bytes_len: usize) {
        self.result = Some((offset, bytes_len));
    }
}

// Syscall C wrappers
impl SyscallContext {
    pub extern "C" fn wrap_write_result(&mut self, offset: u32, bytes_len: u32) {
        self.write_result(offset as usize, bytes_len as usize);
    }
}

pub mod syscall {
    pub const WRITE_RESULT: &str = "write_result";
}

pub type MainFunc = extern "C" fn(&mut SyscallContext);

pub fn register_syscalls(engine: &ExecutionEngine) {
    unsafe {
        engine.register_symbol(
            syscall::WRITE_RESULT,
            SyscallContext::wrap_write_result as *const fn(*mut c_void, *const u8, u64) as *mut (),
        )
    };
}
