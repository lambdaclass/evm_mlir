pub struct SyscallHandler {
    pub result: Vec<u8>,
}

impl SyscallHandler {
    fn write_result(&mut self, bytes: &[u8]) {
        self.result.extend_from_slice(bytes);
    }
}

#[repr(C)]
pub struct SyscallContext<'a> {
    self_ptr: &'a mut SyscallHandler,
}

impl<'a> SyscallContext<'a> {
    pub fn new(self_ptr: &'a mut SyscallHandler) -> Self {
        Self { self_ptr }
    }
}

impl<'a> SyscallContext<'a> {
    pub extern "C" fn wrap_write_result(
        ptr: &mut SyscallHandler,
        bytes_ptr: *const u8,
        bytes_len: u64,
    ) {
        // TODO: verify safety
        let bytes = unsafe { std::slice::from_raw_parts(bytes_ptr, bytes_len as usize) };
        ptr.write_result(bytes);
    }
}

pub mod syscall {
    pub const WRITE_RESULT: &'static str = "write_result";
}

pub type MainFunc = extern "C" fn(&mut SyscallContext);
