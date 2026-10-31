// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Identifies supported file families from a bounded content prefix.
//! Classification is deliberately shallow: signatures and text heuristics provide
//! organization metadata without executing files or trusting filename extensions.
//! Unknown content remains eligible for storage under the fallback classification.

/// Classify content from a bounded prefix without opening or executing it.
pub fn classify(prefix: &[u8]) -> &'static str {
    if prefix.starts_with(b"\x7fELF") {
        return "elf";
    }
    if prefix.starts_with(b"%PDF-") {
        return "pdf";
    }
    if prefix.starts_with(b"PK\x03\x04") || prefix.starts_with(b"PK\x05\x06") {
        return "zip";
    }
    if prefix.len() >= 4
        && matches!(
            &prefix[..4],
            b"\xfe\xed\xfa\xce" | b"\xce\xfa\xed\xfe" | b"\xfe\xed\xfa\xcf" | b"\xcf\xfa\xed\xfe"
        )
    {
        return "macho";
    }
    if let Some(kind) = infer::get(prefix) {
        return match kind.extension() {
            "exe" => "pe",
            "mach" => "macho",
            _ => "other",
        };
    }
    if !prefix.is_empty()
        && std::str::from_utf8(prefix).is_ok_and(|s| {
            s.chars()
                .all(|c| !c.is_control() || matches!(c, '\t' | '\r' | '\n'))
        })
    {
        "text"
    } else {
        "unknown"
    }
}
