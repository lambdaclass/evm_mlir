#![allow(dead_code)]
macro_rules! field_offset {
    ( $ident:path, $field:ident ) => {
        unsafe {
            let value_ptr = std::mem::MaybeUninit::<$ident>::uninit().as_ptr();
            let field_ptr: *const u8 = std::ptr::addr_of!((*value_ptr).$field) as *const u8;
            field_ptr.offset_from(value_ptr as *const u8) as usize
        }
    };
}

pub struct SyscallHandler {
    pub result: Vec<u8>,
}

#[repr(C)]
pub struct SyscallHandlerCallbacks<'a> {
    self_ptr: &'a mut SyscallHandler,
    write_result: extern "C" fn(/*ptr: &mut SyscallHandler, bytes_ptr: *const u8, bytes_len: usize*/),
}

impl SyscallHandler {
    fn write_result(&mut self, bytes: &[u8]) {
        self.result.extend_from_slice(bytes);
    }
}

#[allow(unused_variables)]
impl<'a> SyscallHandlerCallbacks<'a> {
    // Callback field indices.
    pub const WRITE_RESULT: usize = field_offset!(Self, write_result) >> 3;
}

impl<'a> SyscallHandlerCallbacks<'a> {
    pub fn new(handler: &'a mut SyscallHandler) -> Self {
        Self {
            self_ptr: handler,
            write_result: Self::wrap_write_result,
        }
    }

    extern "C" fn wrap_write_result(// ptr: &mut SyscallHandler,
        // bytes_ptr: *const u8,
        // bytes_len: usize,
    ) {
        eprintln!("aaaaaaaaaaaaaaaaa");
        // TODO: verify safety
        // let bytes = unsafe { std::slice::from_raw_parts(bytes_ptr, bytes_len) };
        // ptr.write_result(bytes);
    }
}

pub type MainFunc = extern "C" fn(&mut SyscallHandlerCallbacks);
