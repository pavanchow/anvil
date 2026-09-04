//! Parser for the readable text IR. `printer` (Display) and this module
//! round-trip: `parse_module(&module.to_string()) == module`.

use crate::error::{Error, Result};
use crate::ir::*;

pub fn parse_module(src: &str) -> Result<Module> {
    let raw: Vec<&str> = src.lines().collect();
    let mut functions = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let line = clean(raw[i]);
        if line.is_empty() {
            i += 1;
            continue;
        }
        if line.starts_with("fn ") {
            let (func, next) = parse_function(&raw, i)?;
            functions.push(func);
            i = next;
        } else {
            return Err(Error::Parse(format!("unexpected line {}: {line}", i + 1)));
        }
    }
    Ok(Module { functions })
}

/// Convenience for programs that contain a single function.
pub fn parse_function_str(src: &str) -> Result<Function> {
    let m = parse_module(src)?;
    m.functions
        .into_iter()
        .next()
        .ok_or_else(|| Error::Parse("no function found".into()))
}

fn clean(line: &str) -> &str {
    let line = match line.find(';') {
        Some(i) => &line[..i],
        None => line,
    };
    line.trim()
}

fn parse_vreg(s: &str) -> Result<VReg> {
    let s = s.trim();
    let name = s
        .strip_prefix('%')
        .ok_or_else(|| Error::Parse(format!("expected %reg, got '{s}'")))?;
    if name.is_empty() {
        return Err(Error::Parse("empty register name".into()));
    }
    Ok(VReg::new(name))
}

fn parse_slot(s: &str) -> Result<usize> {
    let n = s
        .trim()
        .strip_prefix("slot")
        .ok_or_else(|| Error::Parse(format!("expected slotN, got '{s}'")))?;
    n.parse()
        .map_err(|_| Error::Parse(format!("bad slot index: '{s}'")))
}

enum Line {
    Inst(Inst),
    Term(Term),
}

fn parse_function(raw: &[&str], start: usize) -> Result<(Function, usize)> {
    let header = clean(raw[start]);
    let header = header
        .strip_prefix("fn ")
        .ok_or_else(|| Error::Parse(format!("expected fn header: {header}")))?;
    let open = header
        .find('(')
        .ok_or_else(|| Error::Parse("missing '(' in function header".into()))?;
    let close = header
        .find(')')
        .ok_or_else(|| Error::Parse("missing ')' in function header".into()))?;
    let name = header[..open].trim().to_string();
    if name.is_empty() {
        return Err(Error::Parse("missing function name".into()));
    }
    let mut params = Vec::new();
    for p in header[open + 1..close].split(',') {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        params.push(parse_vreg(p)?);
    }
    if !header[close + 1..].contains('{') {
        return Err(Error::Parse(format!("expected '{{' in header: {header}")));
    }

    let mut blocks: Vec<BasicBlock> = Vec::new();
    let mut cur_label: Option<Label> = None;
    let mut cur_insts: Vec<Inst> = Vec::new();
    let mut cur_term: Option<Term> = None;
    let mut i = start + 1;
    loop {
        if i >= raw.len() {
            return Err(Error::Parse("unterminated function".into()));
        }
        let line = clean(raw[i]);
        i += 1;
        if line.is_empty() {
            continue;
        }
        if line == "}" {
            if let Some(l) = cur_label.take() {
                let t = cur_term
                    .take()
                    .ok_or_else(|| Error::Parse(format!("block {l} missing terminator")))?;
                blocks.push(BasicBlock {
                    label: l,
                    insts: std::mem::take(&mut cur_insts),
                    term: t,
                });
            }
            break;
        }
        if let Some(lbl) = line.strip_suffix(':') {
            if let Some(l) = cur_label.take() {
                let t = cur_term
                    .take()
                    .ok_or_else(|| Error::Parse(format!("block {l} missing terminator")))?;
                blocks.push(BasicBlock {
                    label: l,
                    insts: std::mem::take(&mut cur_insts),
                    term: t,
                });
            }
            cur_label = Some(Label::new(lbl.trim()));
            cur_insts = Vec::new();
            cur_term = None;
            continue;
        }
        if cur_label.is_none() {
            return Err(Error::Parse(format!("instruction outside a block: {line}")));
        }
        if cur_term.is_some() {
            return Err(Error::Parse(format!("instruction after terminator: {line}")));
        }
        match parse_line(line)? {
            Line::Inst(inst) => cur_insts.push(inst),
            Line::Term(t) => cur_term = Some(t),
        }
    }
    if blocks.is_empty() {
        return Err(Error::Parse(format!("function {name} has no blocks")));
    }
    Ok((Function { name, params, blocks }, i))
}

fn parse_line(line: &str) -> Result<Line> {
    if let Some(r) = line.strip_prefix("jmp ") {
        return Ok(Line::Term(Term::Jmp(Label::new(r.trim()))));
    }
    if let Some(r) = line.strip_prefix("br ") {
        let parts: Vec<&str> = r.split(',').map(str::trim).collect();
        if parts.len() != 3 {
            return Err(Error::Parse(format!("br needs cond, then, else: {line}")));
        }
        return Ok(Line::Term(Term::Br {
            cond: parse_vreg(parts[0])?,
            then_l: Label::new(parts[1]),
            else_l: Label::new(parts[2]),
        }));
    }
    if let Some(r) = line.strip_prefix("ret ") {
        return Ok(Line::Term(Term::Ret(parse_vreg(r.trim())?)));
    }
    if let Some(r) = line.strip_prefix("store ") {
        let parts: Vec<&str> = r.split(',').map(str::trim).collect();
        if parts.len() != 2 {
            return Err(Error::Parse(format!("store needs slot, reg: {line}")));
        }
        return Ok(Line::Inst(Inst::Store {
            slot: parse_slot(parts[0])?,
            src: parse_vreg(parts[1])?,
        }));
    }
    let eq = line
        .find('=')
        .ok_or_else(|| Error::Parse(format!("expected '=' in '{line}'")))?;
    let dst = parse_vreg(line[..eq].trim())?;
    let rhs = line[eq + 1..].trim();
    parse_rhs(dst, rhs).map(Line::Inst)
}

fn parse_rhs(dst: VReg, rhs: &str) -> Result<Inst> {
    if let Some(r) = rhs.strip_prefix("const ") {
        let val: i64 = r
            .trim()
            .parse()
            .map_err(|_| Error::Parse(format!("bad const value: '{r}'")))?;
        return Ok(Inst::Const { dst, val });
    }
    if let Some(r) = rhs.strip_prefix("copy ") {
        return Ok(Inst::Copy {
            dst,
            src: parse_vreg(r.trim())?,
        });
    }
    if let Some(r) = rhs.strip_prefix("load ") {
        return Ok(Inst::Load {
            dst,
            slot: parse_slot(r.trim())?,
        });
    }
    if let Some(r) = rhs.strip_prefix("phi") {
        return parse_phi(dst, r.trim());
    }
    let sp = rhs
        .find(' ')
        .ok_or_else(|| Error::Parse(format!("bad instruction: '{rhs}'")))?;
    let op = BinOp::from_name(&rhs[..sp])
        .ok_or_else(|| Error::Parse(format!("unknown operator: '{}'", &rhs[..sp])))?;
    let operands: Vec<&str> = rhs[sp + 1..].split(',').map(str::trim).collect();
    if operands.len() != 2 {
        return Err(Error::Parse(format!("binop needs two operands: '{rhs}'")));
    }
    Ok(Inst::Bin {
        dst,
        op,
        a: parse_vreg(operands[0])?,
        b: parse_vreg(operands[1])?,
    })
}

fn parse_phi(dst: VReg, r: &str) -> Result<Inst> {
    let inner = r
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| Error::Parse(format!("phi expects [..]: '{r}'")))?;
    let mut args = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let colon = part
            .find(':')
            .ok_or_else(|| Error::Parse(format!("phi arg expects 'label: %reg': '{part}'")))?;
        args.push((
            Label::new(part[..colon].trim()),
            parse_vreg(part[colon + 1..].trim())?,
        ));
    }
    Ok(Inst::Phi { dst, args })
}
