//! Validation and the hand-written-slot fix.
//!
//! Two defect classes are covered: hand-written `load`/`store` must run to a
//! clean error that both interpreters agree on (never a panic), and ill-formed
//! SSA must be rejected by `validate` on every entry point.

mod common;

use anvil::parse::parse_function_str;
use anvil::validate::validate;
use anvil::{run_ir, run_target, Error};

fn func(src: &str) -> anvil::ir::Function {
    parse_function_str(src).expect("parse")
}

// ---- defect 1: hand-written slots ----

#[test]
fn load_from_unset_slot_errors_cleanly_on_both_interpreters() {
    let f = func(
        "\
fn main(%a) {
entry:
  %t = load slot1
  ret %t
}",
    );
    let ir = run_ir(&f, &[9]);
    let tgt = run_target(&f, 2, &[9]);
    assert_eq!(
        ir,
        Err(Error::Runtime("load from unset slot1".into())),
        "IR interp should error on unset-slot load"
    );
    // No panic, and the two backends agree (this is what `check` compares).
    assert_eq!(ir, tgt, "IR and target must agree on unset-slot load");
}

#[test]
fn hand_written_store_then_load_roundtrips_through_the_target() {
    let f = func(
        "\
fn main(%a) {
entry:
  store slot3, %a
  %t = load slot3
  ret %t
}",
    );
    // slot3 is far past any allocator slot; the frame must still be sized for it.
    assert_eq!(run_ir(&f, &[42]).unwrap(), 42);
    assert_eq!(run_target(&f, 2, &[42]).unwrap(), 42);
    assert_eq!(run_target(&f, 16, &[42]).unwrap(), 42);
}

// ---- defect 2: SSA validation ----

fn expect_validate_err(src: &str) -> String {
    match validate(&func(src)) {
        Err(Error::Validate(m)) => m,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_definition() {
    let m = expect_validate_err(
        "\
fn main(%a) {
entry:
  %x = const 1
  %x = const 2
  ret %x
}",
    );
    assert!(m.contains("defined more than once"), "{m}");
}

#[test]
fn rejects_param_redefinition() {
    let m = expect_validate_err(
        "\
fn main(%a) {
entry:
  %a = const 1
  ret %a
}",
    );
    assert!(m.contains("defined more than once"), "{m}");
}

#[test]
fn rejects_undefined_use() {
    let m = expect_validate_err(
        "\
fn main(%a) {
entry:
  ret %undef
}",
    );
    assert!(m.contains("undefined"), "{m}");
}

#[test]
fn rejects_use_before_definition_in_same_block() {
    let m = expect_validate_err(
        "\
fn main(%a) {
entry:
  %y = add %z, %a
  %z = const 1
  ret %y
}",
    );
    assert!(m.contains("before its definition"), "{m}");
}

#[test]
fn rejects_use_not_dominated_by_definition() {
    // %t is defined only on the `then` path but used at the join.
    let m = expect_validate_err(
        "\
fn main(%a) {
entry:
  br %a, then, else
then:
  %t = const 5
  jmp done
else:
  jmp done
done:
  ret %t
}",
    );
    assert!(m.contains("before its definition"), "{m}");
}

#[test]
fn rejects_phi_entry_for_non_predecessor() {
    let m = expect_validate_err(
        "\
fn main(%a) {
entry:
  %c = const 1
  br %a, then, else
then:
  %x = const 10
  jmp done
else:
  %y = const 20
  jmp done
done:
  %p = phi [then: %x, entry: %c]
  ret %p
}",
    );
    assert!(m.contains("not a predecessor"), "{m}");
}

#[test]
fn rejects_phi_missing_predecessor_entry() {
    let m = expect_validate_err(
        "\
fn main(%a) {
entry:
  br %a, then, else
then:
  %x = const 10
  jmp done
else:
  %y = const 20
  jmp done
done:
  %p = phi [then: %x]
  ret %p
}",
    );
    assert!(m.contains("missing an entry"), "{m}");
}

#[test]
fn rejects_missing_jump_target() {
    let m = expect_validate_err(
        "\
fn main(%a) {
entry:
  jmp nowhere
}",
    );
    assert!(m.contains("unknown label"), "{m}");
}

#[test]
fn accepts_all_reference_programs() {
    for (name, src, _) in common::programs_with_args() {
        validate(&func(src)).unwrap_or_else(|e| panic!("valid program {name} rejected: {e}"));
    }
    validate(&func(common::FACT)).unwrap();
    validate(&func(common::DIVZERO)).unwrap();
}

#[test]
fn accepts_hand_written_load_store() {
    validate(&func(
        "\
fn main(%a) {
entry:
  store slot0, %a
  %t = load slot0
  ret %t
}",
    ))
    .unwrap();
}
