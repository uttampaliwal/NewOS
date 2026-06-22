//! Fuzz target for VFS path resolution.
//!
//! Reads lines from stdin and tests the VFS path parser for panics.
//! Tests path normalization, component extraction, and edge cases.

use std::io::{self, BufRead};

fn normalize_path(path: &str) -> Result<String, &'static str> {
    if path.is_empty() {
        return Err("empty path");
    }
    if !path.starts_with('/') {
        return Err("not absolute");
    }

    let mut components: Vec<&str> = Vec::new();

    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                components.pop();
            }
            c => {
                if c.contains('\0') {
                    return Err("null byte in path");
                }
                if c.len() > 255 {
                    return Err("component too long");
                }
                components.push(c);
            }
        }
    }

    if components.is_empty() {
        return Ok(String::from("/"));
    }

    let mut result = String::new();
    for comp in &components {
        result.push('/');
        result.push_str(comp);
    }
    Ok(result)
}

fn resolve_path(base: &str, relative: &str) -> Result<String, &'static str> {
    if relative.starts_with('/') {
        return normalize_path(relative);
    }

    let mut parts: Vec<&str> = base.split('/').collect();
    for component in relative.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            c => parts.push(c),
        }
    }

    let result = parts.join("/");
    if result.is_empty() {
        Ok(String::from("/"))
    } else {
        Ok(result)
    }
}

fn main() {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let _ = normalize_path(&line);

        // Also test path resolution with various bases
        let _ = resolve_path("/", &line);
        let _ = resolve_path("/usr", &line);
        let _ = resolve_path("/usr/bin", &line);
        let _ = resolve_path("/a/b/c/d", &line);

        // Test edge cases
        let _ = normalize_path(&format!("{}{}", line, "\0"));
        let _ = normalize_path(&format!("//{}//", line));
        let _ = resolve_path(&line, "../..");
    }
}
