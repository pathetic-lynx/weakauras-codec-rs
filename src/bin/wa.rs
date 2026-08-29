use std::convert::TryFrom;
use std::fmt::Write as _;
use std::io::{self, Read};
use std::process::ExitCode;

use weakauras_codec::{LuaValue, OutputStringVersion, decode, encode};
use weakauras_codec_lua_value::{LuaMapKey, Map};

fn lua_value_to_json(value: &LuaValue, out: &mut String, indent: usize) {
    let pad = "  ".repeat(indent);
    let inner_pad = "  ".repeat(indent + 1);
    match value {
        LuaValue::Null => out.push_str("null"),
        LuaValue::Boolean(b) => write!(out, "{b}").unwrap(),
        LuaValue::Number(n) => {
            if n.is_finite() {
                if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                    write!(out, "{}", *n as i64).unwrap();
                } else {
                    write!(out, "{n}").unwrap();
                }
            } else {
                out.push_str("null");
            }
        }
        LuaValue::String(s) => write_json_string(s, out),
        LuaValue::Array(arr) => {
            if arr.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, v) in arr.iter().enumerate() {
                out.push_str(&inner_pad);
                lua_value_to_json(v, out, indent + 1);
                if i + 1 < arr.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push(']');
        }
        LuaValue::Map(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            let entries: Vec<_> = map.iter().collect();
            for (i, (k, v)) in entries.iter().enumerate() {
                out.push_str(&inner_pad);
                let key_str = match k.as_value() {
                    LuaValue::String(s) => s.clone(),
                    LuaValue::Number(n) => {
                        if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                            format!("{}", *n as i64)
                        } else {
                            format!("{n}")
                        }
                    }
                    LuaValue::Boolean(b) => format!("{b}"),
                    _ => format!("{:?}", k.as_value()),
                };
                write_json_string(&key_str, out);
                out.push_str(": ");
                lua_value_to_json(v, out, indent + 1);
                if i + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push('}');
        }
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let (mode, version) = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice()
    {
        [_, "decode"] => ("decode", None),
        [_, "encode"] => ("encode", Some(OutputStringVersion::BinarySerialization)),
        [_, "encode", "--deflate"] => ("encode", Some(OutputStringVersion::Deflate)),
        [_, "encode", "--binary"] => ("encode", Some(OutputStringVersion::BinarySerialization)),
        _ => {
            eprintln!("Usage: wa <decode|encode> [--deflate|--binary]");
            eprintln!();
            eprintln!("  decode              Read a WA string from stdin, write JSON to stdout");
            eprintln!("  encode              Read JSON from stdin, write a WA string to stdout");
            eprintln!("    --deflate         Use Deflate encoding (! prefix)");
            eprintln!("    --binary          Use BinarySerialization encoding (!WA:2! prefix, default)");
            return Err("invalid arguments".into());
        }
    };

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let input = input.trim();

    match mode {
        "decode" => {
            let value = decode(input.as_bytes(), None)?.ok_or("decoded value is nil")?;
            let mut json = String::new();
            lua_value_to_json(&value, &mut json, 0);
            println!("{json}");
        }
        "encode" => {
            let value = json_to_lua_value(input)?;
            let encoded = encode(&value, version.unwrap())?;
            println!("{encoded}");
        }
        _ => unreachable!(),
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn json_to_lua_value(input: &str) -> Result<LuaValue, Box<dyn std::error::Error>> {
    let (value, rest) = parse_json_value(input)?;
    let rest = rest.trim();
    if !rest.is_empty() {
        return Err(format!("trailing data: {}", &rest[..rest.len().min(20)]).into());
    }
    Ok(value)
}

fn parse_json_value(input: &str) -> Result<(LuaValue, &str), Box<dyn std::error::Error>> {
    let input = input.trim_start();
    if input.is_empty() {
        return Err("unexpected end of input".into());
    }
    match input.as_bytes()[0] {
        b'"' => parse_json_string(input).map(|(s, r)| (LuaValue::String(s), r)),
        b'{' => parse_json_object(input),
        b'[' => parse_json_array(input),
        b't' if input.starts_with("true") => Ok((LuaValue::Boolean(true), &input[4..])),
        b'f' if input.starts_with("false") => Ok((LuaValue::Boolean(false), &input[5..])),
        b'n' if input.starts_with("null") => Ok((LuaValue::Null, &input[4..])),
        b'-' | b'0'..=b'9' => parse_json_number(input),
        c => Err(format!("unexpected character: '{}'", c as char).into()),
    }
}

fn parse_json_string(input: &str) -> Result<(String, &str), Box<dyn std::error::Error>> {
    debug_assert!(input.starts_with('"'));
    let mut result = String::new();
    let mut chars = input[1..].char_indices();
    loop {
        match chars.next() {
            None => return Err("unterminated string".into()),
            Some((_, '"')) => {
                let rest = chars.as_str();
                return Ok((result, rest));
            }
            Some((_, '\\')) => match chars.next() {
                None => return Err("unterminated escape".into()),
                Some((_, '"')) => result.push('"'),
                Some((_, '\\')) => result.push('\\'),
                Some((_, '/')) => result.push('/'),
                Some((_, 'n')) => result.push('\n'),
                Some((_, 'r')) => result.push('\r'),
                Some((_, 't')) => result.push('\t'),
                Some((_, 'b')) => result.push('\x08'),
                Some((_, 'f')) => result.push('\x0c'),
                Some((_, 'u')) => {
                    let hex: String = (0..4)
                        .map(|_| chars.next().map(|(_, c)| c).ok_or("unterminated \\u escape"))
                        .collect::<Result<_, _>>()?;
                    let code = u16::from_str_radix(&hex, 16)
                        .map_err(|_| format!("invalid \\u escape: {hex}"))?;
                    if let Some(c) = char::from_u32(code as u32) {
                        result.push(c);
                    } else if (0xD800..=0xDBFF).contains(&code) {
                        match (chars.next(), chars.next()) {
                            (Some((_, '\\')), Some((_, 'u'))) => {}
                            _ => return Err("expected low surrogate after high surrogate".into()),
                        }
                        let hex2: String = (0..4)
                            .map(|_| {
                                chars.next().map(|(_, c)| c).ok_or("unterminated \\u escape")
                            })
                            .collect::<Result<_, _>>()?;
                        let low = u16::from_str_radix(&hex2, 16)
                            .map_err(|_| format!("invalid \\u escape: {hex2}"))?;
                        let cp =
                            0x10000 + ((code as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
                        result.push(char::from_u32(cp).ok_or("invalid surrogate pair")?);
                    } else {
                        return Err(format!("lone low surrogate: \\u{hex}").into());
                    }
                }
                Some((_, c)) => return Err(format!("invalid escape: \\{c}").into()),
            },
            Some((_, c)) => result.push(c),
        }
    }
}

fn parse_json_number(input: &str) -> Result<(LuaValue, &str), Box<dyn std::error::Error>> {
    let end = input
        .find(|c: char| !matches!(c, '0'..='9' | '.' | '-' | '+' | 'e' | 'E'))
        .unwrap_or(input.len());
    let num_str = &input[..end];
    let n: f64 = num_str
        .parse()
        .map_err(|_| format!("invalid number: {num_str}"))?;
    Ok((LuaValue::Number(n), &input[end..]))
}

fn parse_json_array(input: &str) -> Result<(LuaValue, &str), Box<dyn std::error::Error>> {
    debug_assert!(input.starts_with('['));
    let mut rest = input[1..].trim_start();
    let mut arr = Vec::new();
    if rest.starts_with(']') {
        return Ok((LuaValue::Array(arr), &rest[1..]));
    }
    loop {
        let (val, r) = parse_json_value(rest)?;
        arr.push(val);
        rest = r.trim_start();
        match rest.as_bytes().first() {
            Some(b',') => rest = rest[1..].trim_start(),
            Some(b']') => return Ok((LuaValue::Array(arr), &rest[1..])),
            _ => return Err("expected ',' or ']'".into()),
        }
    }
}

fn parse_json_object(input: &str) -> Result<(LuaValue, &str), Box<dyn std::error::Error>> {
    debug_assert!(input.starts_with('{'));
    let mut rest = input[1..].trim_start();
    let mut map = Map::new();
    if rest.starts_with('}') {
        return Ok((LuaValue::Map(map), &rest[1..]));
    }
    loop {
        if !rest.starts_with('"') {
            return Err("expected string key".into());
        }
        let (key, r) = parse_json_string(rest)?;
        rest = r.trim_start();
        if !rest.starts_with(':') {
            return Err("expected ':'".into());
        }
        rest = rest[1..].trim_start();
        let (val, r) = parse_json_value(rest)?;
        let map_key = LuaMapKey::try_from(LuaValue::String(key))?;
        map.insert(map_key, val);
        rest = r.trim_start();
        match rest.as_bytes().first() {
            Some(b',') => rest = rest[1..].trim_start(),
            Some(b'}') => return Ok((LuaValue::Map(map), &rest[1..])),
            _ => return Err("expected ',' or '}}'".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_json(value: &LuaValue) -> String {
        let mut out = String::new();
        lua_value_to_json(value, &mut out, 0);
        out
    }

    fn roundtrip(json: &str, version: OutputStringVersion) {
        let value = json_to_lua_value(json).expect("JSON parse failed");
        let wa_string = encode(&value, version).expect("encode failed");
        let decoded = decode(wa_string.as_bytes(), None)
            .expect("decode failed")
            .expect("decoded value is nil");
        assert_eq!(
            to_json(&value),
            to_json(&decoded),
            "roundtrip mismatch for input: {json}"
        );
    }

    #[test]
    fn roundtrip_string() {
        roundtrip(r#""Hello, world!""#, OutputStringVersion::BinarySerialization);
        roundtrip(r#""Hello, world!""#, OutputStringVersion::Deflate);
    }

    #[test]
    fn roundtrip_number() {
        roundtrip("42", OutputStringVersion::BinarySerialization);
        roundtrip("3.14", OutputStringVersion::BinarySerialization);
        roundtrip("-1", OutputStringVersion::Deflate);
    }

    #[test]
    fn roundtrip_boolean_and_null() {
        roundtrip("true", OutputStringVersion::BinarySerialization);
        roundtrip("false", OutputStringVersion::BinarySerialization);
        roundtrip("null", OutputStringVersion::Deflate);
    }

    #[test]
    fn roundtrip_array() {
        roundtrip(r#"[1, "two", true, null]"#, OutputStringVersion::BinarySerialization);
        roundtrip("[]", OutputStringVersion::Deflate);
    }

    #[test]
    fn roundtrip_object() {
        roundtrip(
            r#"{"name": "Test Aura", "enabled": true, "count": 42}"#,
            OutputStringVersion::BinarySerialization,
        );
        roundtrip(r#"{"only": "one"}"#, OutputStringVersion::Deflate);
    }

    #[test]
    fn roundtrip_nested() {
        roundtrip(
            r#"{"outer": {"inner": [1, 2, {"deep": true}]}, "list": [{"a": "b"}]}"#,
            OutputStringVersion::BinarySerialization,
        );
    }

    #[test]
    fn roundtrip_special_chars_in_string() {
        roundtrip(
            r#""line1\nline2\ttab\\backslash\"quote""#,
            OutputStringVersion::BinarySerialization,
        );
    }

    #[test]
    fn decode_known_wa_string() {
        let value = decode(b"!WA:2!JXl5rQ5Kt(6Oq55xuoPOiaa", None)
            .expect("decode failed")
            .expect("nil");
        assert_eq!(value, LuaValue::String("Hello, world!".into()));
        let mut json = String::new();
        lua_value_to_json(&value, &mut json, 0);
        assert_eq!(json, r#""Hello, world!""#);
    }
}
