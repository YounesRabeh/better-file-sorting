//! Terminal presentation, separate from processing and acquisition state.
use crate::app::cli::{ColorMode, Presentation};
use std::{
    io::IsTerminal,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
static PROGRESS: AtomicBool = AtomicBool::new(false);
static COLOR: AtomicBool = AtomicBool::new(false);

pub fn color_enabled(
    mode: ColorMode,
    no_color: bool,
    json: bool,
    tty: bool,
    env_no_color: bool,
) -> bool {
    !json
        && !no_color
        && match mode {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => tty && !env_no_color,
        }
}
pub fn initialize(args: &Presentation) {
    COLOR.store(
        color_enabled(
            args.color,
            args.no_color,
            args.json,
            std::io::stderr().is_terminal(),
            std::env::var_os("NO_COLOR").is_some(),
        ),
        Ordering::Relaxed,
    );
    PROGRESS.store(
        !args.quiet && !args.json && std::io::stderr().is_terminal(),
        Ordering::Relaxed,
    );
}
pub fn label(text: &str, code: &str) -> String {
    if COLOR.load(Ordering::Relaxed) {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.into()
    }
}
/// One shared aggregate bar per operation. Drop always clears it, including early errors.
pub struct Progress(pub indicatif::ProgressBar);
impl Progress {
    pub fn new(total: Option<u64>, operation: &str) -> Self {
        let bar = if PROGRESS.load(Ordering::Relaxed) {
            match total {
                Some(n) => indicatif::ProgressBar::new(n),
                None => indicatif::ProgressBar::new_spinner(),
            }
        } else {
            indicatif::ProgressBar::hidden()
        };
        bar.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(8));
        if !PROGRESS.load(Ordering::Relaxed) {
            bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }
        bar.set_style(
            indicatif::ProgressStyle::with_template(match (total.is_some(), COLOR.load(Ordering::Relaxed)) {
                (true, true) => "{prefix} [{wide_bar:.cyan/blue}] {pos}/{len} [{elapsed_precise}] {per_sec} {msg}",
                (true, false) => "{prefix} [{wide_bar}] {pos}/{len} [{elapsed_precise}] {per_sec} {msg}",
                (false, true) => "{prefix} {spinner:.cyan} [{elapsed_precise}] {msg}",
                (false, false) => "{prefix} {spinner} [{elapsed_precise}] {msg}",
            })
            .expect("static progress template").progress_chars("=>-"),
        );
        bar.set_prefix(operation.to_owned());
        bar.enable_steady_tick(Duration::from_millis(125));
        Self(bar)
    }
}
impl Drop for Progress {
    fn drop(&mut self) {
        self.0.finish_and_clear();
    }
}

/// Escape remote control and bidi characters before inserting names into terminal text.
pub fn safe_text(value: &str) -> String {
    value.chars().flat_map(|c| {
        if c.is_control() || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            c.escape_default().collect::<Vec<_>>()
        } else { vec![c] }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_precedence() {
        assert_eq!(safe_text("a\x1b[31m\n.zip"), "a\\u{1b}[31m\\n.zip");
        assert!(!color_enabled(ColorMode::Auto, false, false, false, false));
        assert!(!color_enabled(ColorMode::Auto, false, false, true, true));
        assert!(color_enabled(ColorMode::Auto, false, false, true, false));
        assert!(color_enabled(ColorMode::Always, false, false, false, true));
        assert!(!color_enabled(ColorMode::Always, true, false, true, false));
        assert!(!color_enabled(ColorMode::Always, false, true, true, false));
        assert!(!color_enabled(ColorMode::Never, false, false, true, false));
    }
}

/// Binary byte units for human output; serialized byte fields remain integers.
pub fn bytes(value: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if value < 1024 {
        return format!("{value} B");
    }
    let mut number = value as f64;
    let mut unit = 0;
    while number >= 1024.0 && unit < UNITS.len() - 1 {
        number /= 1024.0;
        unit += 1;
    }
    format!("{number:.2} {}", UNITS[unit])
}

pub fn stats(repository: &std::path::Path, summary: &crate::output::report::Summary) -> String {
    use std::fmt::Write;
    let mismatches = summary
        .by_mismatch_status
        .get("mismatch")
        .map_or(0, |v| v.occurrences);
    let mut text = format!(
        "Repository  {}\nSamples     {} unique / {} occurrences\nDuplicates  {} occurrences\nStored      {}\nLogical     {}\nMismatches  {} occurrences\n",
        safe_text(&repository.display().to_string()),
        summary.unique_samples,
        summary.occurrences,
        summary.duplicate_occurrences,
        bytes(summary.stored_bytes),
        bytes(summary.logical_bytes),
        mismatches
    );
    if !summary.nested.occurrences_by_depth.is_empty()
        || !summary.nested.archive_outcomes.is_empty()
    {
        let _ = write!(
            text,
            "\nNested archive analysis\n  Parent-child links: {}\n  Maximum sample depth: {}\n",
            summary.nested.parent_child_relationships, summary.nested.maximum_depth
        );
        for (status, count) in &summary.nested.archive_outcomes {
            let _ = writeln!(text, "  {:24} {}", safe_text(status), count);
        }
        for (depth, count) in &summary.nested.occurrences_by_depth {
            let _ = writeln!(
                text,
                "  Depth {depth}: {count} occurrences / {} unique hashes",
                summary.nested.unique_hashes_by_depth[depth]
            );
        }
    }
    for (title, groups) in [
        ("Detected formats", &summary.by_detected_format),
        ("Detected families", &summary.by_detected_family),
        ("Mismatch statuses", &summary.by_mismatch_status),
    ] {
        let _ = write!(
            text,
            "\n{title}\n  {:24} {:>12} {:>13}\n",
            "Value", "Occurrences", "Unique hashes"
        );
        for (name, counts) in groups {
            let _ = writeln!(
                text,
                "  {:24} {:>12} {:>13}",
                safe_text(name),
                counts.occurrences,
                counts.unique_hashes
            );
        }
    }
    text
}

#[cfg(test)]
mod formatting_tests {
    #[test]
    fn binary_byte_boundaries() {
        for (value, expected) in [
            (0, "0 B"),
            (512, "512 B"),
            (1024, "1.00 KiB"),
            (1048576, "1.00 MiB"),
            (u64::MAX, "16.00 EiB"),
        ] {
            assert_eq!(super::bytes(value), expected);
        }
    }
}
