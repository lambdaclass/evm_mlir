use std::collections::BTreeMap;

use melior::{
    dialect::cf,
    ir::{Block, BlockRef, Location, Value},
    Context as MeliorContext,
};

use crate::{errors::CodegenError, program::Program, syscall};

#[derive(Debug, Clone)]
pub(crate) struct OperationCtx<'c> {
    /// The MLIR context.
    pub mlir_context: &'c MeliorContext,
    /// The program IR.
    pub program: &'c Program,
    /// The syscall context to be passed to syscalls.
    pub syscall_ctx: Value<'c, 'c>,
    /// Reference to the revert block.
    /// This block takes care of reverts.
    pub revert_block: BlockRef<'c, 'c>,
    /// Reference to the jump table block.
    /// This block receives the PC as an argument and jumps to the block corresponding to that PC,
    /// or reverts in case the destination is not a JUMPDEST.
    pub jumptable_block: BlockRef<'c, 'c>,
    /// Blocks to jump to. These are registered dynamically as JUMPDESTs are processed.
    pub jumpdest_blocks: BTreeMap<usize, BlockRef<'c, 'c>>,
}

impl<'c> OperationCtx<'c> {
    /// Registers a block as a valid jump destination.
    // TODO: move into jumptable module
    pub(crate) fn register_jump_destination(&mut self, pc: usize, block: BlockRef<'c, 'c>) {
        self.jumpdest_blocks.insert(pc, block);
    }

    /// Registers a block as a valid jump destination.
    // TODO: move into jumptable module
    #[allow(dead_code)]
    pub(crate) fn add_jump_op(
        &mut self,
        block: BlockRef<'c, 'c>,
        pc_to_jump_to: Value,
        location: Location,
    ) {
        let op = block.append_operation(cf::br(&self.jumptable_block, &[pc_to_jump_to], location));
        assert!(op.verify());
    }
}

// Syscall MLIR wrappers
impl<'c> OperationCtx<'c> {
    pub(crate) fn write_result_syscall(
        &self,
        block: &Block,
        offset: Value,
        size: Value,
        location: Location,
    ) {
        syscall::mlir::write_result_syscall(
            self.mlir_context,
            self.syscall_ctx,
            block,
            offset,
            size,
            location,
        )
    }

    pub(crate) fn extend_memory_syscall(
        &'c self,
        block: &'c Block,
        new_size: Value<'c, 'c>,
        location: Location<'c>,
    ) -> Result<Value, CodegenError> {
        syscall::mlir::extend_memory_syscall(
            self.mlir_context,
            self.syscall_ctx,
            block,
            new_size,
            location,
        )
    }
}
