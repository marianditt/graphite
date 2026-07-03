use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::Diagnostic;
use crate::Severity;

/// Scans source files for `@graphite:evidence` annotations.
///
/// Three comment syntaxes are supported:
/// - `// @graphite:evidence <id>` (line-comment languages)
/// - `# @graphite:evidence <id>`  (hash-comment languages)
/// - `<!-- @graphite:evidence <id> -->` (HTML/XML)
pub struct AnchorScanner;

/// The result of scanning a single root for evidence anchors.
pub type EvidenceMap = HashMap<String, Vec<(PathBuf, usize)>>;

#[allow(clippy::result_large_err)]
// Extensions where each comment style is tried.
const EXTENSIONS: &[&str] = &[
    "rs",
    "ts",
    "tsx",
    "js",
    "jsx",
    "go",
    "c",
    "cpp",
    "h",
    "hpp",
    "java",
    "kt",
    "py",
    "rb",
    "sh",
    "yaml",
    "yml",
    "toml",
    "dockerfile",
    "html",
    "xml",
    "md",
];

impl AnchorScanner {
    /// Recursively scan `root` for `@graphite:evidence` annotations.
    ///
    /// Returns a map from evidence ID to list of (file_path, 1-based line_number).
    /// If `root` is a file, scans only that file. If it's a directory, walks
    /// recursively (depth-first).
    ///
    /// # Errors
    /// Returns a `Diagnostic` with `rule = "scan-error"` if the root does not
    /// exist or cannot be read.
    #[allow(clippy::result_large_err)]
    pub fn scan(root: &Path) -> Result<EvidenceMap, Diagnostic> {
        if !root.exists() {
            return Err(Diagnostic {
                rule: "scan-error".into(),
                severity: Severity::Error,
                node_id: None,
                file: Some(root.to_string_lossy().into()),
                detail: format!("Scan root does not exist: {}", root.display()),
                fix: "Provide a valid directory or file path.".into(),
                example: None,
                hint: "The scan root must exist and be readable.".into(),
            });
        }

        let mut map: EvidenceMap = HashMap::new();

        if root.is_dir() {
            Self::walk_dir(root, &mut map).map_err(|e| Diagnostic {
                rule: "scan-error".into(),
                severity: Severity::Error,
                node_id: None,
                file: Some(root.to_string_lossy().into()),
                detail: format!(
                    "Failed to scan directory '{}': {}",
                    root.display(),
                    e.detail
                ),
                fix: "Check that the directory exists and is readable.".into(),
                example: None,
                hint: "Ensure the scan root is a readable directory.".into(),
            })?;
        } else {
            Self::scan_file(root, &mut map)?;
        }

        Ok(map)
    }

    /// Recursively walk a directory and scan all supported files.
    #[allow(clippy::result_large_err)]
    fn walk_dir(dir: &Path, map: &mut EvidenceMap) -> Result<(), Diagnostic> {
        let entries = fs::read_dir(dir).map_err(|e| Diagnostic {
            rule: "scan-error".into(),
            severity: Severity::Error,
            node_id: None,
            file: Some(dir.to_string_lossy().into()),
            detail: format!("Cannot read directory '{}': {}", dir.display(), e),
            fix: "Ensure the directory is readable.".into(),
            example: None,
            hint: "Directory must exist and be readable.".into(),
        })?;

        for entry in entries {
            let path = entry
                .map_err(|e| Diagnostic {
                    rule: "scan-error".into(),
                    severity: Severity::Error,
                    node_id: None,
                    file: Some(dir.to_string_lossy().into()),
                    detail: format!("Directory entry error: {e}"),
                    fix: "Check filesystem permissions.".into(),
                    example: None,
                    hint: "All directory entries must be readable.".into(),
                })?
                .path();

            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && (name.starts_with('.') || name == "node_modules" || name == "target")
                {
                    continue;
                }
                Self::walk_dir(&path, map)?;
            } else if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str())
                    && EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
                {
                    Self::scan_file(&path, map)?;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && name.eq_ignore_ascii_case("dockerfile")
                {
                    Self::scan_file(&path, map)?;
                }
            }
        }

        Ok(())
    }

    /// Scan a single file for `@graphite:evidence` annotations using all three
    /// comment syntaxes. Results are appended to `map` — the caller is
    /// responsible for any post-processing (e.g. deduplication or ambiguity
    /// checks).
    #[allow(clippy::result_large_err)]
    fn scan_file(path: &Path, map: &mut EvidenceMap) -> Result<(), Diagnostic> {
        let content = fs::read_to_string(path).map_err(|e| Diagnostic {
            rule: "scan-error".into(),
            severity: Severity::Error,
            node_id: None,
            file: Some(path.to_string_lossy().into()),
            detail: format!("Cannot read file '{}': {}", path.display(), e),
            fix: "Ensure the file exists and is a valid UTF-8 text file.".into(),
            example: None,
            hint: "Only UTF-8 text files can contain evidence annotations.".into(),
        })?;

        for (line_idx, line) in content.lines().enumerate() {
            let line_number = line_idx + 1; // 1-based
            let trimmed = line.trim();

            // Try each comment style.
            if let Some(id) = Self::extract_line_comment(trimmed, "// @graphite:evidence ") {
                map.entry(id)
                    .or_default()
                    .push((path.to_path_buf(), line_number));
                continue;
            }
            if let Some(id) = Self::extract_line_comment(trimmed, "# @graphite:evidence ") {
                map.entry(id)
                    .or_default()
                    .push((path.to_path_buf(), line_number));
                continue;
            }
            if let Some(id) = Self::extract_html_comment(trimmed) {
                map.entry(id)
                    .or_default()
                    .push((path.to_path_buf(), line_number));
            }
        }

        Ok(())
    }

    /// Check if `line` starts with `prefix`. If so, extract the evidence ID
    /// (the rest of the line, trimmed).
    fn extract_line_comment(line: &str, prefix: &str) -> Option<String> {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(prefix) {
            let id = rest.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        None
    }

    /// Extract an evidence ID from an HTML-style comment:
    /// `<!-- @graphite:evidence <id> -->`
    fn extract_html_comment(line: &str) -> Option<String> {
        let marker = "<!-- @graphite:evidence ";
        let line = line.trim();
        if let Some(start) = line.find(marker) {
            let after = &line[start + marker.len()..];
            if let Some(end) = after.find("-->") {
                let id = after[..end].trim();
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// Create a temporary file with the given content and return its path.
    fn write_temp(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(&path).unwrap();
        write!(f, "{content}").unwrap();
        path
    }

    #[test]
    fn finds_line_comment_evidence() {
        let dir = TempDir::new().unwrap();
        write_temp(
            &dir,
            "main.rs",
            "// @graphite:evidence auth-service\nfn main() {}\n",
        );

        let map = AnchorScanner::scan(dir.path()).unwrap();
        assert_eq!(map.len(), 1);
        let locations = map.get("auth-service").unwrap();
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].1, 1);
    }

    #[test]
    fn finds_hash_comment_evidence() {
        let dir = TempDir::new().unwrap();
        write_temp(
            &dir,
            "app.py",
            "# @graphite:evidence login-flow\ndef login(): pass\n",
        );

        let map = AnchorScanner::scan(dir.path()).unwrap();
        let locations = map.get("login-flow").unwrap();
        assert_eq!(locations.len(), 1);
    }

    #[test]
    fn finds_html_comment_evidence() {
        let dir = TempDir::new().unwrap();
        write_temp(
            &dir,
            "page.html",
            "<!-- @graphite:evidence homepage -->\n<body></body>\n",
        );

        let map = AnchorScanner::scan(dir.path()).unwrap();
        let locations = map.get("homepage").unwrap();
        assert_eq!(locations.len(), 1);
    }

    #[test]
    fn collects_multiple_occurrences_of_same_id() {
        let dir = TempDir::new().unwrap();
        write_temp(&dir, "a.rs", "// @graphite:evidence shared\nfn a() {}\n");
        write_temp(&dir, "b.rs", "// @graphite:evidence shared\nfn b() {}\n");

        let map = AnchorScanner::scan(dir.path()).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("shared").unwrap().len(), 2);
    }

    #[test]
    fn empty_dir_returns_empty_map() {
        let dir = TempDir::new().unwrap();
        let map = AnchorScanner::scan(dir.path()).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn non_existent_dir_returns_error() {
        let result = AnchorScanner::scan(Path::new("/nonexistent/path"));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().rule, "scan-error");
    }

    #[test]
    fn skips_hidden_and_binary_dirs() {
        let dir = TempDir::new().unwrap();
        // Create a file inside a hidden dir.
        write_temp(&dir, ".hidden/foo.rs", "// @graphite:evidence hidden\n");
        // Create a normal file.
        write_temp(&dir, "visible.rs", "// @graphite:evidence visible\n");

        let map = AnchorScanner::scan(dir.path()).unwrap();
        assert!(map.get("hidden").is_none(), "should not scan hidden dirs");
        assert!(map.get("visible").is_some());
    }

    #[test]
    fn file_without_evidence_returns_empty() {
        let dir = TempDir::new().unwrap();
        write_temp(&dir, "main.rs", "fn main() { println!(\"hello\"); }\n");
        let map = AnchorScanner::scan(dir.path()).unwrap();
        assert!(map.is_empty());
    }
}
