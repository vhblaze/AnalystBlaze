use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Prevents spawned console programs (ping, netsh, powershell, sc.exe, ...)
/// from flashing a visible CMD window on top of the app.
pub trait CommandExt {
    fn no_window(&mut self) -> &mut Command;
}

impl CommandExt for Command {
    #[cfg(windows)]
    fn no_window(&mut self) -> &mut Command {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(windows))]
    fn no_window(&mut self) -> &mut Command {
        self
    }
}

/// Decodes captured stdout/stderr bytes from a spawned Windows console tool
/// (netsh, sc, powercfg, wmic, ...). These legacy console programs write in
/// the system's OEM codepage (e.g. CP850/CP860 on a pt-BR Windows install),
/// not UTF-8 - decoding them as UTF-8 unconditionally corrupts any accented
/// character (service display names, Wi-Fi SSIDs, localized power plan
/// names such as "Equilibrado") into mojibake/replacement characters.
///
/// UTF-8 is tried first since it's already correct for the common
/// ASCII-only case and for tools that do emit UTF-8.
#[cfg(windows)]
pub fn decode_console_bytes(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    let codepage = unsafe { windows::Win32::Globalization::GetOEMCP() };
    let decoded = decode_console_bytes_with_codepage(bytes, codepage);
    if looks_like_mojibake(&decoded) {
        // The OEM codepage guess still didn't produce clean text - log it
        // instead of silently showing garbled output, so a codepage this
        // hasn't been seen on yet is at least diagnosable from the field.
        let _ = crate::audit::record_event(
            "warning",
            "system.console_decode_failed",
            "Saida de um comando do Windows nao pode ser decodificada corretamente.",
            serde_json::json!({ "codepage": codepage }),
        );
    }
    decoded
}

#[cfg(not(windows))]
pub fn decode_console_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

/// Codepage-explicit decode, split out from decode_console_bytes so it's
/// deterministic and unit-testable without depending on the current
/// system's OEM codepage.
#[cfg(windows)]
pub fn decode_console_bytes_with_codepage(bytes: &[u8], codepage: u32) -> String {
    use windows::Win32::Globalization::{MultiByteToWideChar, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};

    if bytes.is_empty() {
        return String::new();
    }

    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    let required = unsafe { MultiByteToWideChar(codepage, flags, bytes, None) };
    if required <= 0 {
        return String::from_utf8_lossy(bytes).to_string();
    }

    let mut wide = vec![0u16; required as usize];
    let written = unsafe { MultiByteToWideChar(codepage, flags, bytes, Some(&mut wide)) };
    if written <= 0 {
        return String::from_utf8_lossy(bytes).to_string();
    }

    String::from_utf16_lossy(&wide[..written as usize])
}

/// True if `text` contains the Unicode replacement character, the
/// telltale sign that a byte sequence failed to decode cleanly under
/// whatever encoding was used - the automated mojibake regression guard
/// for decode_console_bytes.
pub fn looks_like_mojibake(text: &str) -> bool {
    text.contains('\u{FFFD}')
}

/// True when an error's text indicates the file is currently open/locked
/// by another process (Windows' ERROR_SHARING_VIOLATION, os error 32).
/// String-matched against the error's own Display output rather than a
/// dedicated ErrorKind - std::io::Error has no such variant on stable
/// Rust, and some callers (snapshot.rs's copy_dir_recursive) have already
/// stringified the original io::Error by the time this runs, so matching
/// on `&str` covers both a fresh io::Error's .to_string() and an
/// already-flattened error message alike. Shared by disk_usage.rs (large
/// permanent deletes) and snapshot.rs (the much more common quarantine-move
/// path): a real report (2026-09) was that deleting a file currently in
/// use didn't clearly say why. The permanent-delete path already had this
/// classification; the quarantine path - what most everyday deletes go
/// through - never applied it and just surfaced a raw, compound
/// rename/copy failure string instead.
/// Shared wording for every delete/move path that classifies this case -
/// disk_usage.rs's two delete paths and snapshot.rs's cross-volume moves.
pub const FILE_IN_USE_MESSAGE: &str =
    "o item esta em uso por outro programa - feche-o e tente novamente.";

pub fn error_indicates_file_in_use(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("being used by another process") || lower.contains("em uso por outro processo")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Encodes `text` into `codepage` bytes via WideCharToMultiByte, the
    /// inverse of decode_console_bytes_with_codepage - lets tests round-trip
    /// through a real Windows codepage table instead of hand-typing byte
    /// values from memory.
    fn encode_with_codepage(text: &str, codepage: u32) -> Vec<u8> {
        use windows::Win32::Globalization::{WideCharToMultiByte, WC_COMPOSITECHECK};

        let wide: Vec<u16> = text.encode_utf16().collect();
        let required = unsafe {
            WideCharToMultiByte(codepage, WC_COMPOSITECHECK, &wide, None, None, None)
        };
        assert!(required > 0, "encoding setup failed for the test itself");

        let mut bytes = vec![0u8; required as usize];
        let written = unsafe {
            WideCharToMultiByte(codepage, WC_COMPOSITECHECK, &wide, Some(&mut bytes), None, None)
        };
        assert!(written > 0, "encoding setup failed for the test itself");
        bytes.truncate(written as usize);
        bytes
    }

    const CP850_LATIN1_MULTILINGUAL: u32 = 850;

    #[test]
    fn decodes_accented_text_from_a_non_utf8_codepage() {
        let original = "café com ação e não";
        let cp850_bytes = encode_with_codepage(original, CP850_LATIN1_MULTILINGUAL);

        // Sanity check: this really isn't valid UTF-8, otherwise the test
        // would pass for the wrong reason (the UTF-8 fast path).
        assert!(std::str::from_utf8(&cp850_bytes).is_err());

        let decoded = decode_console_bytes_with_codepage(&cp850_bytes, CP850_LATIN1_MULTILINGUAL);
        assert_eq!(decoded, original);
        assert!(!looks_like_mojibake(&decoded));
    }

    #[test]
    fn naive_utf8_decoding_of_cp850_bytes_would_have_been_mojibake() {
        let original = "café";
        let cp850_bytes = encode_with_codepage(original, CP850_LATIN1_MULTILINGUAL);
        let naive = String::from_utf8_lossy(&cp850_bytes).to_string();
        assert!(looks_like_mojibake(&naive) || naive != original);
    }

    #[test]
    fn ascii_only_output_passes_through_unchanged() {
        let bytes = b"RUNNING\r\nSTATE: 4\r\n";
        assert_eq!(decode_console_bytes(bytes), "RUNNING\r\nSTATE: 4\r\n");
    }

    #[test]
    fn already_utf8_output_is_not_reencoded() {
        let bytes = "já em UTF-8: 100%".as_bytes();
        assert_eq!(decode_console_bytes(bytes), "já em UTF-8: 100%");
    }

    #[test]
    fn empty_input_decodes_to_empty_string() {
        assert_eq!(decode_console_bytes_with_codepage(&[], CP850_LATIN1_MULTILINGUAL), "");
    }

    /// Regression test for the 2026-09 report: deleting a file open in
    /// another program didn't clearly say why. The English case is
    /// verified against this exact machine's real
    /// std::io::Error::from_raw_os_error(32) text - Windows' own
    /// FormatMessage came back in English here even on a pt-BR install, so
    /// the Portuguese substring below is asserted only against the exact
    /// wording this function already checks for, not a guessed full
    /// sentence this session couldn't verify against a real pt-BR OS
    /// message.
    #[test]
    fn recognizes_file_in_use_errors_in_either_language() {
        assert!(error_indicates_file_in_use(
            "The process cannot access the file because it is being used by another process. (os error 32)"
        ));
        assert!(error_indicates_file_in_use(
            "O processo nao pode acessar o arquivo porque ele esta em uso por outro processo. (os error 32)"
        ));
        assert!(!error_indicates_file_in_use("Access is denied. (os error 5)"));
    }
}
