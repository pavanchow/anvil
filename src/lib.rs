//! Anvil: a from-scratch compiler backend.
//!
//! It takes an SSA IR and lowers it to a small target machine, and its headline
//! feature is a readable graph-coloring register allocator. Correctness is
//! machine-checkable: [`run_ir`] interprets the IR over unlimited virtual
//! registers, [`run_target`] interprets the final lowered code over only K
//! physical registers plus spill slots, and they must agree.

pub mod error;
pub mod interference;
pub mod interp;
pub mod ir;
pub mod liveness;
pub mod lower;
pub mod parse;
pub mod regalloc;
pub mod ssa;
pub mod target;

pub use error::{Error, Result};

/// Full backend: SSA destruction, register allocation, lowering to target code.
pub fn compile(func: &ir::Function, k: usize) -> Result<target::TFunc> {
    let phi_free = ssa::destruct(func);
    let alloc = regalloc::allocate(&phi_free, k)?;
    Ok(lower::lower(&alloc))
}

/// Run the reference interpreter (the oracle) on the SSA IR.
pub fn run_ir(func: &ir::Function, args: &[i64]) -> Result<i64> {
    interp::run(func, args)
}

/// Compile with K registers and run the resulting target code.
pub fn run_target(func: &ir::Function, k: usize, args: &[i64]) -> Result<i64> {
    let tf = compile(func, k)?;
    target::run(&tf, args)
}
