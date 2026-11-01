//! Compare filename extensions with independent content evidence.
use crate::detection::classifier::{Confidence, Detection, Format};
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum Status {
    #[default]
    NotEvaluated,
    Match,
    Compatible,
    Mismatch,
    MissingExtension,
    UnknownExtension,
    UnknownFormat,
    Ambiguous,
}
impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotEvaluated => "not_evaluated",
            Self::Match => "match",
            Self::Compatible => "compatible",
            Self::Mismatch => "mismatch",
            Self::MissingExtension => "missing_extension",
            Self::UnknownExtension => "unknown_extension",
            Self::UnknownFormat => "unknown_format",
            Self::Ambiguous => "ambiguous",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evaluation {
    pub status: Status,
    pub reason: String,
}
impl Default for Evaluation {
    fn default() -> Self {
        Self {
            status: Status::NotEvaluated,
            reason: "legacy record; analysis has not been requested".into(),
        }
    }
}
pub fn normalize_extension(extension: &str) -> String {
    extension.trim_start_matches('.').to_ascii_lowercase()
}
pub fn extension(filename: &str) -> String {
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or("");
    let lower = basename.to_ascii_lowercase();
    for ext in ["tar.gz", "tar.bz2", "tar.xz"] {
        if lower.ends_with(&format!(".{ext}")) {
            return ext.into();
        }
    }
    normalize_extension(
        std::path::Path::new(basename)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or(""),
    )
}
fn formats(extension: &str) -> Option<&'static [Format]> {
    use Format::*;
    Some(match extension {
        "exe" | "dll" | "sys" | "scr" | "ocx" | "cpl" => &[Pe],
        "elf" | "so" => &[Elf],
        "dylib" | "macho" => &[Macho],
        "pdf" => &[Pdf],
        "rtf" => &[Rtf],
        "doc" | "xls" | "ppt" | "msi" => &[Cfb],
        "docx" | "docm" => &[Docx],
        "xlsx" | "xlsm" => &[Xlsx],
        "pptx" | "pptm" => &[Pptx],
        "js" | "mjs" | "cjs" => &[Javascript],
        "ps1" | "psm1" => &[Powershell],
        "py" => &[Python],
        "sh" | "bash" | "zsh" => &[Shell],
        "bat" | "cmd" => &[Batch],
        "html" | "htm" => &[Html],
        "xml" | "svg" => &[Xml],
        "json" => &[Json],
        "txt" | "log" | "csv" | "md" => &[Text],
        "zip" => &[Zip],
        "rar" => &[Rar],
        "7z" => &[SevenZip],
        "tar" => &[Tar],
        "gz" | "tgz" | "tar.gz" => &[Gzip],
        "bz2" | "tbz2" | "tar.bz2" => &[Bzip2],
        "xz" | "txz" | "tar.xz" => &[Xz],
        "jar" => &[Jar],
        "apk" => &[Apk],
        "png" => &[Png],
        "jpg" | "jpeg" | "jpe" => &[Jpeg],
        "gif" => &[Gif],
        "webp" => &[Webp],
        "bmp" => &[Bmp],
        "tif" | "tiff" => &[Tiff],
        "sqlite" | "sqlite3" | "db" => &[Sqlite],
        "wasm" => &[Wasm],
        _ => return None,
    })
}
pub fn evaluate(filename: &str, detection: &Detection) -> Evaluation {
    use Status::*;
    let ext = extension(filename);
    let format = detection.detected_format;
    let (status, reason) = if format == Format::NotEvaluated {
        (NotEvaluated, "content has not been evaluated")
    } else if ext.is_empty() {
        (MissingExtension, "filename has no extension")
    } else if format == Format::Ambiguous {
        (Ambiguous, "conflicting content interpretations")
    } else if format == Format::Unknown || detection.confidence == Confidence::Unknown {
        (UnknownFormat, "insufficient content evidence")
    } else if matches!(ext.as_str(), "bin" | "dat" | "tmp") {
        (Compatible, "generic extension imposes no specific format")
    } else if let Some(expected) = formats(&ext) {
        if expected.contains(&format) {
            (Match, "extension is registered for the detected format")
        } else if format == Format::Zip
            && expected.iter().any(|f| {
                matches!(
                    f,
                    Format::Docx | Format::Xlsx | Format::Pptx | Format::Jar | Format::Apk
                )
            })
        {
            (Ambiguous, "ZIP container subtype could not be established")
        } else if ext == "zip"
            && matches!(
                format,
                Format::Docx | Format::Xlsx | Format::Pptx | Format::Jar | Format::Apk
            )
        {
            (Compatible, "identified format uses a ZIP container")
        } else if detection.confidence < Confidence::High {
            (
                Ambiguous,
                "content evidence is insufficient for a definitive mismatch",
            )
        } else {
            (
                Mismatch,
                "strong content evidence contradicts the registered extension",
            )
        }
    } else {
        (
            UnknownExtension,
            "extension is not in the compatibility registry",
        )
    };
    Evaluation {
        status,
        reason: reason.into(),
    }
}
