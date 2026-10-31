//! Terminal presentation, separate from processing and acquisition state.
use crate::cli::{ColorMode, Presentation};
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
            indicatif::ProgressStyle::with_template(if total.is_some() {
                "{prefix} [{wide_bar}] {pos}/{len} {msg}"
            } else {
                "{prefix} {spinner} {msg}"
            })
            .expect("static progress template"),
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
