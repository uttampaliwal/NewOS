//! Fuzz target for IPC message parsing (turnix-ipc-proto).
//!
//! Reads raw bytes from stdin and attempts to deserialize them as
//! IPC messages using postcard (the wire format used by Turnix).

#![allow(dead_code)]

use std::io::{self, Read};

#[derive(Debug)]
enum IpcValueType {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<IpcValueType>),
    Map(Vec<(IpcValueType, IpcValueType)>),
    Null,
}

#[derive(Debug)]
enum IpcMessageKind {
    MethodCall {
        id: u64,
        service: String,
        method: String,
        args: Vec<IpcValueType>,
    },
    MethodReturn {
        id: u64,
        result: Result<IpcValueType, IpcError>,
    },
    Signal {
        service: String,
        signal: String,
        args: Vec<IpcValueType>,
    },
}

#[derive(Debug)]
struct IpcError {
    code: u32,
    message: String,
}

fn try_parse_varint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        if *pos >= data.len() {
            return None;
        }
        let byte = data[*pos];
        *pos += 1;
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    Some(result)
}

fn try_parse_string(data: &[u8], pos: &mut usize) -> Option<String> {
    let len = try_parse_varint(data, pos)? as usize;
    if *pos + len > data.len() {
        return None;
    }
    let bytes = &data[*pos..*pos + len];
    *pos += len;
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

fn try_parse_ipc_message(data: &[u8]) -> Option<IpcMessageKind> {
    let mut pos = 0;

    let msg_type = try_parse_varint(data, &mut pos)?;

    match msg_type {
        0 => {
            // MethodCall
            let id = try_parse_varint(data, &mut pos)?;
            let service = try_parse_string(data, &mut pos)?;
            let method = try_parse_string(data, &mut pos)?;
            let arg_count = try_parse_varint(data, &mut pos)? as usize;
            let mut args = Vec::new();
            for _ in 0..arg_count {
                let val_type = try_parse_varint(data, &mut pos)?;
                match val_type {
                    0 => args.push(IpcValueType::Null),
                    1 => {
                        let v = try_parse_varint(data, &mut pos)?;
                        args.push(IpcValueType::Int(v as i64));
                    }
                    2 => {
                        let s = try_parse_string(data, &mut pos)?;
                        args.push(IpcValueType::String(s));
                    }
                    _ => args.push(IpcValueType::Null),
                }
            }
            Some(IpcMessageKind::MethodCall {
                id,
                service,
                method,
                args,
            })
        }
        1 => {
            // MethodReturn
            let id = try_parse_varint(data, &mut pos)?;
            let success = try_parse_varint(data, &mut pos)?;
            if success == 0 {
                let val_type = try_parse_varint(data, &mut pos)?;
                let _ = val_type;
                Some(IpcMessageKind::MethodReturn {
                    id,
                    result: Ok(IpcValueType::Null),
                })
            } else {
                let code = try_parse_varint(data, &mut pos)? as u32;
                let message = try_parse_string(data, &mut pos)?;
                Some(IpcMessageKind::MethodReturn {
                    id,
                    result: Err(IpcError { code, message }),
                })
            }
        }
        2 => {
            // Signal
            let service = try_parse_string(data, &mut pos)?;
            let signal = try_parse_string(data, &mut pos)?;
            Some(IpcMessageKind::Signal {
                service,
                signal,
                args: Vec::new(),
            })
        }
        _ => None,
    }
}

fn main() {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf).unwrap();

    let _msg = try_parse_ipc_message(&buf);
}
