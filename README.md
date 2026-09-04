# Anvil

Anvil is a compiler backend built from scratch in Rust with zero external
dependencies. It takes an SSA intermediate representation and lowers it to code
for a small target machine, and its headline feature is a readable graph
coloring register allocator.

Most build your own compiler projects stop at the front end. They parse a
language, build an AST, maybe walk a tree to interpret it, and finish there. The
backend is the part they skip, and register allocation is the part of the
backend they skip hardest, because it is where correctness gets genuinely hard
to see by eye. Anvil fills that gap and then proves it is correct.

## The idea that makes it trustworthy

Anvil ships two interpreters.

1. An IR interpreter that runs the SSA program directly over an unlimited supply
   of virtual registers. This is the reference, the oracle.
2. A target interpreter that runs the final lowered program after register
   allocation, over only K physical registers plus a stack of spill slots.

For every test program, and for small K that force values out of registers and
onto the stack, Anvil asserts:

```
interp_ir(program, args) == interp_target(lower(regalloc(program, K)), args)
```

If those two ever disagree, the backend is wrong. That single equation is the
spine of the project. Register allocation, spilling, phi elimination, and
lowering are all validated by it at K=2, K=3, and a K large enough to need no
spilling at all.

## The pipeline

```
text IR ──parse──► SSA IR ──destruct──► phi-free IR ──liveness──►
   interference graph ──regalloc (Chaitin-Briggs)──► colored IR ──lower──► target code
```

Each stage lives in its own small module.

- `ir.rs` the SSA types and the readable text form, plus the shared arithmetic
  both interpreters call so their math agrees by construction.
- `parse.rs` a parser for the text IR that round-trips with the printer.
- `interp.rs` the reference interpreter (the oracle).
- `ssa.rs` SSA destruction. Phi nodes become copies in predecessors, critical
  edges are split, and the parallel copies for one edge are sequenced so a swap
  cycle does not clobber.
- `liveness.rs` backward dataflow liveness over the control flow graph.
- `interference.rs` the interference graph. Two values interfere when one is
  live at the other's definition.
- `regalloc.rs` Chaitin-Briggs graph coloring. Build, simplify, optimistic
  potential spill, select, and actual spill with reload code inserted around
  every use and definition.
- `lower.rs` and `target.rs` the small target machine, its printer, and its
  interpreter.

## The IR

```
fn main(%a, %b) {
entry:
  %0 = const 10
  %1 = add %a, %0
  br %1, then, else
then:
  %2 = mul %1, %b
  jmp done
else:
  %3 = const 0
  jmp done
done:
  %4 = phi [then: %2, else: %3]
  ret %4
}
```

Instructions are `const`, the binary operators
`add sub mul div mod and or xor shl shr eq ne lt le gt ge`, `copy`, and `phi`.
Terminators are `jmp`, `br`, and `ret`. Division or modulo by zero is a defined
runtime error and both interpreters produce it identically.

## Using it

```
cargo build
cargo test
cargo clippy --all-targets

anvil run      program.ir              # interpret the IR and print the result
anvil run      program.ir 5            # pass function arguments
anvil regalloc program.ir --regs 2     # show liveness, interference, coloring, spills
anvil emit     program.ir --regs 2     # print the lowered target assembly
anvil check    program.ir --regs 2 100 # run the round-trip oracle and report OK or mismatch
```

A sample round-trip at K=2 on the sum-of-1..n program, where register pressure
forces spilling to the stack:

```
$ anvil check sum.ir --regs 2 100
IR interpreter:     5050
target interpreter: 5050
registers: 2, spills: 7, slots: 7
OK: results match
```

## Live playground

`docs/index.html` is a self-contained page that ports the whole pipeline to
JavaScript. Enter IR, pick K, and watch liveness, the interference graph, the
coloring and spills, and the emitted target code, with a check that the target
result matches the reference interpreter. It mirrors the Rust behavior.

## Companion projects

Anvil is the third piece of a from-scratch toolchain by the same author.
Alchemist is the compiler front end. Whetstone is the optimizer. Anvil is the
backend.

Author: Pavan Nallamothu
