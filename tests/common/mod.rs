//! Shared IR programs and helpers for the integration tests. Each test binary
//! links this module and uses a subset, so unused items here are expected.
#![allow(dead_code)]

pub const ADD: &str = "\
fn main(%a, %b) {
entry:
  %0 = add %a, %b
  ret %0
}";

/// Branch and phi: returns a*b when a+10 is truthy, else 0.
pub const BRANCH: &str = "\
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
}";

/// Sum of 1..=n via a loop. High register pressure: n, the constant one, the
/// loop counter and the accumulator are all live across the loop.
pub const SUM: &str = "\
fn main(%n) {
entry:
  %zero = const 0
  %one = const 1
  jmp loop
loop:
  %i = phi [entry: %one, body: %i2]
  %acc = phi [entry: %zero, body: %acc2]
  %cond = le %i, %n
  br %cond, body, done
body:
  %acc2 = add %acc, %i
  %i2 = add %i, %one
  jmp loop
done:
  ret %acc
}";

/// Factorial of n via a loop.
pub const FACT: &str = "\
fn main(%n) {
entry:
  %one = const 1
  jmp loop
loop:
  %i = phi [entry: %one, body: %i2]
  %acc = phi [entry: %one, body: %acc2]
  %cond = le %i, %n
  br %cond, body, done
body:
  %acc2 = mul %acc, %i
  %i2 = add %i, %one
  jmp loop
done:
  ret %acc
}";

/// Iterative Fibonacci through branches and phis. Returns fib(n).
pub const FIB: &str = "\
fn main(%n) {
entry:
  %zero = const 0
  %one = const 1
  jmp loop
loop:
  %i = phi [entry: %zero, body: %i2]
  %a = phi [entry: %zero, body: %b]
  %b = phi [entry: %one, body: %ab]
  %cond = lt %i, %n
  br %cond, body, done
body:
  %ab = add %a, %b
  %i2 = add %i, %one
  jmp loop
done:
  ret %a
}";

/// Two phis that swap each iteration. Exercises the parallel-copy cycle path in
/// SSA destruction: without a temporary, sequential copies would clobber.
pub const SWAP: &str = "\
fn main(%n) {
entry:
  %one = const 1
  %x0 = const 10
  %y0 = const 20
  jmp loop
loop:
  %i = phi [entry: %one, body: %i2]
  %x = phi [entry: %x0, body: %y]
  %y = phi [entry: %y0, body: %x]
  %cond = le %i, %n
  br %cond, body, done
body:
  %i2 = add %i, %one
  jmp loop
done:
  ret %x
}";

pub const DIVZERO: &str = "\
fn main(%a) {
entry:
  %z = const 0
  %r = div %a, %z
  ret %r
}";

pub fn programs_with_args() -> Vec<(&'static str, &'static str, Vec<Vec<i64>>)> {
    vec![
        ("add", ADD, vec![vec![2, 3], vec![-4, 9], vec![100, -100]]),
        ("branch", BRANCH, vec![vec![0, 7], vec![-10, 5], vec![3, 4]]),
        ("sum", SUM, vec![vec![0], vec![1], vec![5], vec![10], vec![50]]),
        ("fact", FACT, vec![vec![0], vec![1], vec![5], vec![10]]),
        ("fib", FIB, vec![vec![0], vec![1], vec![7], vec![10], vec![20]]),
        ("swap", SWAP, vec![vec![0], vec![1], vec![2], vec![7]]),
    ]
}
