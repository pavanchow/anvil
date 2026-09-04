//! The text IR parses and prints back to an identical module.

mod common;

use anvil::parse::parse_module;

fn roundtrip(src: &str) {
    let m1 = parse_module(src).expect("parse 1");
    let printed = m1.to_string();
    let m2 = parse_module(&printed).expect("parse 2");
    assert_eq!(m1, m2, "AST changed across print/parse\n{printed}");
    // Printing is idempotent.
    assert_eq!(printed, m2.to_string());
}

#[test]
fn text_roundtrips() {
    for (name, src, _) in common::programs_with_args() {
        eprintln!("roundtrip {name}");
        roundtrip(src);
    }
    roundtrip(common::FACT);
    roundtrip(common::DIVZERO);
}

#[test]
fn roundtrip_with_spill_instructions() {
    // Load/Store are printable and parseable too.
    let src = "\
fn main(%a) {
entry:
  store slot0, %a
  %t = load slot0
  ret %t
}";
    roundtrip(src);
}
