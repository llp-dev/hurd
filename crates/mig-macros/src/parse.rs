//! Token-stream parser for the `routine!` grammar.
//!
//! Grammar (BNF-ish):
//!
//!   routine    := "fn" IDENT "=" INT ";" "in" "{" arg_list "}" "out" "{" arg_list "}"
//!   arg_list   := ( IDENT ":" type_tag ";" )*
//!   type_tag   := IDENT  // one of: int, port_send, port_send_poly, mach_port_t
//!
//! The first `in` argument MUST be of tag `mach_port_t` and represents
//! the destination port (msgh_remote_port). It is not placed in the
//! message body; the remaining `in` args are.

use proc_macro::{TokenStream, TokenTree, Delimiter, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeTag {
    /// 32-bit signed int. Wire slot = 8 bytes (descriptor 8 + value 4 + pad 4 = 16 total inline).
    Int,
    /// Inline port name, fixed COPY_SEND disposition.
    PortSend,
    /// Inline port name, caller-supplied disposition (adds an extra `xxxPoly: c_int` arg).
    PortSendPoly,
    /// Special marker for the request-port arg. Goes in msgh_remote_port, not the body.
    MachPortT,
}

impl TypeTag {
    pub fn from_ident(name: &str, span: Span) -> Result<Self, (Span, String)> {
        match name {
            "int"             => Ok(TypeTag::Int),
            "port_send"       => Ok(TypeTag::PortSend),
            "port_send_poly"  => Ok(TypeTag::PortSendPoly),
            "mach_port_t"     => Ok(TypeTag::MachPortT),
            other => Err((span, format!("unknown type tag `{}`", other))),
        }
    }
}

#[derive(Debug)]
pub struct Arg {
    pub name: String,
    pub tag:  TypeTag,
}

#[derive(Debug)]
pub struct ParsedRoutine {
    pub fname:    String,
    pub msgh_id:  i32,
    pub target:   String,        // name of the destination-port arg
    pub in_args:  Vec<Arg>,      // body-only (excludes target)
    pub out_args: Vec<Arg>,
}

/// Parse a `routine!` invocation. On error, returns a `compile_error!`
/// TokenStream the caller can emit directly.
pub fn parse(input: TokenStream) -> Result<ParsedRoutine, TokenStream> {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let mut i = 0;

    // fn IDENT
    expect_ident(&tokens, &mut i, "fn")?;
    let fname = take_ident(&tokens, &mut i)?;

    // = INT ;
    expect_punct(&tokens, &mut i, '=')?;
    let msgh_id = take_int(&tokens, &mut i)?;
    expect_punct(&tokens, &mut i, ';')?;

    // in { ... }
    expect_ident(&tokens, &mut i, "in")?;
    let in_body = take_group(&tokens, &mut i, Delimiter::Brace)?;
    let raw_in = parse_arg_list(in_body)?;

    // out { ... }
    expect_ident(&tokens, &mut i, "out")?;
    let out_body = take_group(&tokens, &mut i, Delimiter::Brace)?;
    let out_args = parse_arg_list(out_body)?;

    if i < tokens.len() {
        return Err(err("unexpected trailing tokens after `out { ... }`"));
    }

    // First `in` arg must be the target (mach_port_t).
    if raw_in.is_empty() {
        return Err(err("`in { ... }` must declare at least the target port (e.g. `target: mach_port_t;`)"));
    }
    if raw_in[0].tag != TypeTag::MachPortT {
        return Err(err("first `in` arg must have type `mach_port_t` (the request port)"));
    }
    let target = raw_in[0].name.clone();
    let in_args: Vec<Arg> = raw_in.into_iter().skip(1).collect();

    // No mach_port_t allowed after the first slot.
    if in_args.iter().any(|a| a.tag == TypeTag::MachPortT) {
        return Err(err("only the first `in` arg may have type `mach_port_t`"));
    }
    if out_args.iter().any(|a| a.tag == TypeTag::MachPortT) {
        return Err(err("`mach_port_t` tag is for the request port and not valid in `out { ... }`; use `port_send`"));
    }

    Ok(ParsedRoutine { fname, msgh_id, target, in_args, out_args })
}

fn parse_arg_list(body: TokenStream) -> Result<Vec<Arg>, TokenStream> {
    let tokens: Vec<TokenTree> = body.into_iter().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < tokens.len() {
        let name = take_ident(&tokens, &mut i)?;
        expect_punct(&tokens, &mut i, ':')?;
        let span = tokens.get(i).map(|t| t.span()).unwrap_or_else(Span::call_site);
        let tag_name = take_ident(&tokens, &mut i)?;
        let tag = TypeTag::from_ident(&tag_name, span)
            .map_err(|(_s, m)| err(&m))?;
        expect_punct(&tokens, &mut i, ';')?;
        out.push(Arg { name, tag });
    }
    Ok(out)
}

// ---- token-stream helpers ----

fn expect_ident(tokens: &[TokenTree], i: &mut usize, expected: &str) -> Result<(), TokenStream> {
    match tokens.get(*i) {
        Some(TokenTree::Ident(ident)) if ident.to_string() == expected => {
            *i += 1;
            Ok(())
        }
        _ => Err(err(&format!("expected `{}`", expected))),
    }
}

fn take_ident(tokens: &[TokenTree], i: &mut usize) -> Result<String, TokenStream> {
    match tokens.get(*i) {
        Some(TokenTree::Ident(ident)) => {
            *i += 1;
            Ok(ident.to_string())
        }
        _ => Err(err("expected identifier")),
    }
}

fn take_int(tokens: &[TokenTree], i: &mut usize) -> Result<i32, TokenStream> {
    match tokens.get(*i) {
        Some(TokenTree::Literal(lit)) => {
            let s = lit.to_string();
            *i += 1;
            s.parse::<i32>().map_err(|_| err("expected integer literal"))
        }
        _ => Err(err("expected integer literal")),
    }
}

fn expect_punct(tokens: &[TokenTree], i: &mut usize, ch: char) -> Result<(), TokenStream> {
    match tokens.get(*i) {
        Some(TokenTree::Punct(p)) if p.as_char() == ch => {
            *i += 1;
            Ok(())
        }
        _ => Err(err(&format!("expected `{}`", ch))),
    }
}

fn take_group(tokens: &[TokenTree], i: &mut usize, delim: Delimiter) -> Result<TokenStream, TokenStream> {
    match tokens.get(*i) {
        Some(TokenTree::Group(g)) if g.delimiter() == delim => {
            *i += 1;
            Ok(g.stream())
        }
        _ => Err(err("expected `{ ... }` block")),
    }
}

fn err(msg: &str) -> TokenStream {
    format!("compile_error!({:?});", msg).parse().unwrap()
}
