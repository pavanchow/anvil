# Design

This document explains how Anvil works and why it is built the way it is. The
guiding constraint is that correctness must be machine checkable, not argued.

## The gap this fills

A compiler front end turns source text into an intermediate representation. An
optimizer rewrites that representation into a better version of itself. A backend
turns the representation into instructions for a real machine, which has a fixed
and small number of registers rather than the unlimited supply the earlier
stages assume. Deciding which values live in which registers, and what to do when
there are not enough, is register allocation. It is the heart of a backend and it
is the stage most learning projects never reach.

Anvil is a backend whose whole reason to exist is to make register allocation
visible and provably correct.

## Why two interpreters

The problem with a register allocator is that a wrong one usually still produces
plausible looking code. It runs. It returns a number. The number is just wrong,
and only on some inputs, and only when spilling kicks in. Reading the output by
eye does not catch this.

So Anvil never trusts the eye. It defines correctness as an equation between two
interpreters.

- The IR interpreter runs the SSA program over a map from virtual register to
  value. There is no register limit and no stack. It is obviously correct
  because it is a direct reading of the IR semantics. This is the oracle.
- The target interpreter runs the final program over an array of exactly K
  registers and an array of spill slots. It is what the backend actually
  produces.

If register allocation, spilling, and lowering are correct, the two agree for
every program and every input. If they disagree, something in the backend is
wrong. Tests assert the equation at K=2 and K=3, which are small enough to force
spilling on the loop programs, and at a large K where nothing spills. The tests
also assert that spilling really does happen at K=2, so the spill path is
actually exercised rather than silently skipped.

One detail makes the equation meaningful rather than circular. Both interpreters
call the same arithmetic function in `ir.rs`. That means the two never disagree
about what `add` or `div` means, including overflow and division by zero. The
equation therefore tests the thing under test, which is allocation and lowering,
and not accidental differences in arithmetic.

## SSA and phi elimination

The IR is in single static assignment form. Every virtual register is written
once. Control flow joins use phi nodes, which pick a value based on which
predecessor block execution came from.

Real machines have no phi instruction, so phi nodes must become ordinary copies
placed in the predecessor blocks. Two things make this subtle and both are
handled in `ssa.rs`.

Critical edges come first. If a predecessor has several successors and the block
with the phi has several predecessors, there is no single safe place to put the
copy. The copy would run on paths it should not. Anvil splits such an edge by
inserting a new block that only jumps to the successor, giving the copy a home.

Parallel copies come second. All the phi nodes at the top of a block take their
values at the same instant, so the copies for one incoming edge happen in
parallel. A cycle such as two phis that swap two values would clobber if the
copies ran naively in sequence. The classic case is `a = b` and `b = a` at once,
which sequentialized as `a = b; b = a` loses the old `a`. Anvil sequences a
parallel copy set by repeatedly emitting any copy whose destination is not still
needed as a source, and when only a cycle remains it saves one value in a fresh
temporary to break it. The `swap` test program relies on this.

Because the IR interpreter also runs phi-free code, SSA destruction is checked by
running the oracle before and after and asserting the results match.

## Liveness and interference

Register allocation needs to know which values are alive at the same time.
`liveness.rs` computes this with standard backward dataflow over the control flow
graph, iterating live-in and live-out sets to a fixed point.

`interference.rs` turns liveness into a graph. Two virtual registers interfere,
meaning they cannot share a physical register, when one is live at the point the
other is defined. Parameters arrive together in distinct registers, so they form
a clique. A copy is a small exception. Its source and destination hold the same
value at that moment, so they are allowed to share a register and no edge is
added between them.

## Register allocation

`regalloc.rs` implements Chaitin-Briggs graph coloring with K colors, one per
physical register.

- Build the interference graph.
- Simplify. Repeatedly remove any node with fewer than K neighbors and push it on
  a stack. Such a node can always be colored later, whatever its neighbors get.
- Potential spill. When every remaining node has K or more neighbors, pick one as
  a spill candidate and push it optimistically. It might still be colorable.
- Select. Pop the stack and give each node a color its neighbors have not used.
- Actual spill. If a node has no free color, it is spilled for real.

When there are actual spills, the code is rewritten. Each spilled value gets a
stack slot. Every use is preceded by a load from the slot into a fresh
temporary, and every definition is followed by a store. The whole allocation then
runs again on the rewritten code. This terminates because reload temporaries have
tiny live ranges and are easy to color, so pressure falls with each round.

Two rules keep it terminating and correct at small K.

- A spilled parameter has no defining instruction to store after, so its incoming
  value is stored to its slot at the very top of the entry block. The parameter
  keeps a one instruction live range there and is colored to the register the
  argument arrives in.
- A temporary introduced for a spill is never spilled again. If such a temporary
  still cannot be colored, then a single instruction needs more simultaneously
  live values than there are registers, K is genuinely too small, and Anvil
  reports that rather than looping.

The strategy is deliberately simple. It favors correctness and readability over
minimizing the number of spills, and at very small K it may spill generously. The
round-trip oracle guarantees the result is always right, and a smarter spill cost
heuristic could be dropped in without changing the interface.

## Lowering and the target machine

`target.rs` defines a small machine. It has K registers named r0 through r(K-1),
a stack of spill slots, immediate moves, register moves, the same binary
operators as the IR, loads and stores against slots, and jump, branch, and return
terminators.

`lower.rs` walks the fully colored IR and maps each instruction to a target
instruction, replacing every virtual register with its color. After allocation
every value has a color, so this is nearly one to one. Copies that ended up
between the same physical register are dropped.

The target interpreter in `target.rs` executes this and returns a value, which is
the second half of the round-trip equation.

## Testing

- Text round-trip. Parsing then printing then parsing again yields an identical
  module.
- Interpreter correctness. Arithmetic, branches, a loop that sums 1..n,
  factorial, and Fibonacci through branches and phis.
- Phi elimination. The oracle agrees before and after SSA destruction, including
  the swap cycle.
- The headline round-trip. The IR interpreter and the target interpreter agree
  across K=2, K=3, and a large K, with an explicit check that spilling occurs at
  K=2 and does not at large K.

Author: Pavan Nallamothu
