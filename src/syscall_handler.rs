use std::ffi::c_void;

use melior::ExecutionEngine;

#[derive(Debug, Default)]
pub struct SyscallContext {
    pub memory: Vec<u8>,
    result: Option<(usize, usize)>,
}

impl SyscallContext {
    pub fn result(&self) -> &[u8] {
        let (offset, size) = self.result.unwrap_or((0, 0));
        &self.memory[offset..offset + size]
    }
}

// Syscall implementations
impl SyscallContext {
    pub extern "C" fn wrap_write_result(&mut self, offset: u32, bytes_len: u32) {
        self.result = Some((offset as usize, bytes_len as usize));
    }

    pub extern "C" fn wrap_extend_memory(&mut self, new_size: u32) -> *mut u8 {
        let new_size = new_size as usize;
        if new_size > self.memory.len() {
            // TODO: check for OOM
            self.memory.resize(new_size, 0);
        }
        self.memory.as_mut_ptr()
    }
}

pub mod syscall {
    pub const WRITE_RESULT: &str = "emv_mlir__write_result";
    pub const EXTEND_MEMORY: &str = "emv_mlir__extend_memory";
}

pub type MainFunc = extern "C" fn(&mut SyscallContext);

pub fn register_syscalls(engine: &ExecutionEngine) {
    unsafe {
        engine.register_symbol(
            syscall::WRITE_RESULT,
            SyscallContext::wrap_write_result as *const fn(*mut c_void, u32, u32) as *mut (),
        );
        engine.register_symbol(
            syscall::EXTEND_MEMORY,
            SyscallContext::wrap_extend_memory as *const fn(*mut c_void, u32) as *mut (),
        );
    };
}
