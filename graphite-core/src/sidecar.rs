use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::Diagnostic;
use crate::Severity;

// @graphite:evidence spec-sidecar
/// Resolves evidence anchors from `.graphite` sidecar files.
///
/// Sidecar files map evidence IDs to patterns (substring or JSONPath) that
/// locate the evidence in the corresponding source file. The sidecar lives
/// alongside the source: `source.ext.graphite`.
///
/// # Format
/// ```json
/// { "anchors": { "<id>": { "pattern": "<substring_or_jsonpath>" } } }
/// ```
///
/// # Pattern types
/// - Patterns starting with `$.` or `$[` are JSONPath navigations on JSON files.
/// - All other patterns are treated as substring matching against lines.
pub struct SidecarResolver;

/// A resolved anchor location.
pub type SidecarMap = HashMap<String, Vec<(PathBuf, usize)>>;

#[derive(Deserialize)]
struct SidecarFile {
    anchors: HashMap<String, AnchorEntry>,
}

#[derive(Deserialize)]
struct AnchorEntry {
    pattern: String,
}

impl SidecarResolver {
    /// Given a source file path, find its `.graphite` sidecar (if it exists)
    /// and resolve all anchors within it.
    ///
    /// Returns a map from evidence ID to resolved `(file_path, 1-based line_number)`.
    ///
    /// # Errors
    /// Returns a `Diagnostic` if the sidecar exists but cannot be parsed, or
    /// if a pattern fails to resolve.
    #[allow(clippy::result_large_err)]
    pub fn resolve(source_path: &Path) -> Result<SidecarMap, Diagnostic> {
        let sidecar = Self::find_sidecar(source_path);
        let sidecar = match sidecar {
            Some(p) => p,
            None => return Ok(HashMap::new()),
        };

        let content = fs::read_to_string(&sidecar).map_err(|e| Diagnostic {
            rule: "sidecar-parse-error".into(),
            severity: Severity::Error,
            node_id: None,
            file: Some(sidecar.to_string_lossy().into()),
            detail: format!("Cannot read sidecar '{}': {}", sidecar.display(), e),
            fix: "Ensure the .graphite sidecar file exists and is readable.".into(),
            example: None,
            hint: "Sidecar files are JSON and must be valid UTF-8.".into(),
        })?;

        let parsed: SidecarFile = serde_json::from_str(&content).map_err(|e| Diagnostic {
            rule: "sidecar-parse-error".into(),
            severity: Severity::Error,
            node_id: None,
            file: Some(sidecar.to_string_lossy().into()),
            detail: format!("Invalid JSON in sidecar '{}': {}", sidecar.display(), e),
            fix: "Fix the JSON syntax in the sidecar file.".into(),
            example: Some(r#"{"anchors": {"my-id": {"pattern": "TODO"}}}"#.into()),
            hint: "Sidecar files must be valid JSON with an 'anchors' object.".into(),
        })?;

        let mut map: SidecarMap = HashMap::new();

        for (evidence_id, entry) in &parsed.anchors {
            let locations = Self::resolve_pattern(source_path, &entry.pattern)?;
            if locations.is_empty() {
                return Err(Diagnostic {
                    rule: "sidecar-unresolved-pattern".into(),
                    severity: Severity::Error,
                    node_id: None,
                    file: Some(source_path.to_string_lossy().into()),
                    detail: format!(
                        "Pattern '{}' for evidence '{}' did not match anything in '{}'",
                        entry.pattern,
                        evidence_id,
                        source_path.display()
                    ),
                    fix: format!(
                        "Update the pattern in '{}' to match content in '{}'.",
                        sidecar.display(),
                        source_path.display()
                    ),
                    example: None,
                    hint: "Substring patterns match line content. JSONPath patterns navigate JSON structure.".into(),
                });
            }
            if locations.len() > 1 {
                return Err(Diagnostic {
                    rule: "sidecar-ambiguous-pattern".into(),
                    severity: Severity::Error,
                    node_id: None,
                    file: Some(source_path.to_string_lossy().into()),
                    detail: format!(
                        "Pattern '{}' for evidence '{}' matched {} locations (expected exactly 1)",
                        entry.pattern,
                        evidence_id,
                        locations.len()
                    ),
                    fix: format!(
                        "Refine the pattern in '{}' to match exactly one location.",
                        sidecar.display()
                    ),
                    example: None,
                    hint: "Sidecar patterns must resolve to exactly one location.".into(),
                });
            }
            map.entry(evidence_id.clone())
                .or_default()
                .extend(locations);
        }

        Ok(map)
    }

    /// Find a `.graphite` sidecar for the given source file.
    ///
    /// Convention: `foo/bar.rs` → `foo/bar.rs.graphite`
    pub fn find_sidecar(source_path: &Path) -> Option<PathBuf> {
        let mut sidecar = source_path.to_path_buf();
        let name = sidecar.file_name()?;
        let mut name_str = name.to_str()?.to_string();
        name_str.push_str(".graphite");
        sidecar.set_file_name(name_str);
        if sidecar.exists() {
            Some(sidecar)
        } else {
            None
        }
    }

    // ------------------------------------------------------------------
    // Pattern resolution
    // ------------------------------------------------------------------

    #[allow(clippy::result_large_err)]
    fn resolve_pattern(
        source_path: &Path,
        pattern: &str,
    ) -> Result<Vec<(PathBuf, usize)>, Diagnostic> {
        let content = match fs::read_to_string(source_path) {
            Ok(c) => c,
            Err(e) => {
                return Err(Diagnostic {
                    rule: "sidecar-source-error".into(),
                    severity: Severity::Error,
                    node_id: None,
                    file: Some(source_path.to_string_lossy().into()),
                    detail: format!("Cannot read source file '{}': {}", source_path.display(), e),
                    fix: "Ensure the source file exists and is a valid UTF-8 text file.".into(),
                    example: None,
                    hint: "Source files must be readable UTF-8 text.".into(),
                });
            }
        };

        if pattern.starts_with("$.") || pattern.starts_with("$[") {
            Self::resolve_jsonpath(&content, pattern, source_path)
        } else {
            Self::resolve_substring(&content, pattern, source_path)
        }
    }

    /// Resolve a simple JSONPath expression against a JSON source file.
    ///
    /// Supports: `$.key`, `$.key.subkey`, `$.key[0]`, `$.key[0].sub`
    #[allow(clippy::result_large_err)]
    fn resolve_jsonpath(
        content: &str,
        pattern: &str,
        source_path: &Path,
    ) -> Result<Vec<(PathBuf, usize)>, Diagnostic> {
        let value: serde_json::Value = serde_json::from_str(content).map_err(|e| Diagnostic {
            rule: "sidecar-source-error".into(),
            severity: Severity::Error,
            node_id: None,
            file: Some(source_path.to_string_lossy().into()),
            detail: format!(
                "JSONPath pattern '{}' used but '{}' is not valid JSON: {}",
                pattern,
                source_path.display(),
                e
            ),
            fix: "Use a substring pattern for non-JSON files, or fix the JSON.".into(),
            example: None,
            hint: "JSONPath patterns can only be used on .json files.".into(),
        })?;

        // Normalize: strip leading "$." or "$" prefix
        let path_str = pattern
            .strip_prefix("$.")
            .or_else(|| pattern.strip_prefix('$'))
            .unwrap_or(pattern);

        // Navigate: split on '.' for keys and '[N]' for array indices
        let mut current = &value;
        let mut segment_start = 0;
        let chars: Vec<char> = path_str.chars().collect();

        while segment_start < chars.len() {
            if chars[segment_start] == '[' {
                // Array index: [N]
                let close = chars[segment_start..]
                    .iter()
                    .position(|&c| c == ']')
                    .ok_or_else(|| Diagnostic {
                        rule: "sidecar-pattern-error".into(),
                        severity: Severity::Error,
                        node_id: None,
                        file: Some(source_path.to_string_lossy().into()),
                        detail: format!("Unclosed bracket in JSONPath: {pattern}"),
                        fix: "Fix the JSONPath syntax.".into(),
                        example: None,
                        hint: "Array access requires [index] syntax.".into(),
                    })?
                    + segment_start;
                let index_str: String = chars[segment_start + 1..close].iter().collect();
                let index: usize = index_str.parse().map_err(|_| Diagnostic {
                    rule: "sidecar-pattern-error".into(),
                    severity: Severity::Error,
                    node_id: None,
                    file: Some(source_path.to_string_lossy().into()),
                    detail: format!("Invalid array index in JSONPath: {index_str}"),
                    fix: "Use a valid integer for array index.".into(),
                    example: None,
                    hint: "Array indices must be non-negative integers.".into(),
                })?;
                current = current.get(index).ok_or_else(|| Diagnostic {
                    rule: "sidecar-unresolved-pattern".into(),
                    severity: Severity::Error,
                    node_id: None,
                    file: Some(source_path.to_string_lossy().into()),
                    detail: format!("Array index {index} out of bounds in JSONPath: {pattern}"),
                    fix: "Correct the array index in the pattern.".into(),
                    example: None,
                    hint: "The JSON array must have at least index+1 elements.".into(),
                })?;
                segment_start = close + 1;
                // Skip following '.' if present
                if segment_start < chars.len() && chars[segment_start] == '.' {
                    segment_start += 1;
                }
            } else {
                // Object key
                let mut end = segment_start;
                while end < chars.len() && chars[end] != '.' && chars[end] != '[' {
                    end += 1;
                }
                let key: String = chars[segment_start..end].iter().collect();
                if !key.is_empty() {
                    current = current.get(&key).ok_or_else(|| Diagnostic {
                        rule: "sidecar-unresolved-pattern".into(),
                        severity: Severity::Error,
                        node_id: None,
                        file: Some(source_path.to_string_lossy().into()),
                        detail: format!("Key '{}' not found in JSON for pattern: {pattern}", key),
                        fix: "Correct the key name in the JSONPath pattern.".into(),
                        example: None,
                        hint: "The key must exist in the JSON structure.".into(),
                    })?;
                }
                segment_start = end + 1; // skip '.'
            }
        }

        // Now we have the target value. Find its line number by scanning.
        let target_str = match current {
            serde_json::Value::String(s) => {
                serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
            }
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            other => other.to_string(),
        };

        // Find the first occurrence in the raw text to get line number
        if let Some(pos) = content.find(&target_str) {
            let line = content[..pos].matches('\n').count() + 1;
            Ok(vec![(source_path.to_path_buf(), line)])
        } else {
            Err(Diagnostic {
                rule: "sidecar-unresolved-pattern".into(),
                severity: Severity::Error,
                node_id: None,
                file: Some(source_path.to_string_lossy().into()),
                detail: format!(
                    "Could not locate JSON value in file content for pattern: {pattern}"
                ),
                fix: "The JSONPath resolves but the resulting value was not found in the file."
                    .into(),
                example: None,
                hint: "Check that the file content matches the expected structure.".into(),
            })
        }
    }

    #[allow(clippy::result_large_err)]
    fn resolve_substring(
        content: &str,
        pattern: &str,
        source_path: &Path,
    ) -> Result<Vec<(PathBuf, usize)>, Diagnostic> {
        if pattern.is_empty() {
            return Err(Diagnostic {
                rule: "sidecar-pattern-error".into(),
                severity: Severity::Error,
                node_id: None,
                file: Some(source_path.to_string_lossy().into()),
                detail: "Empty pattern string".into(),
                fix: "Provide a non-empty pattern.".into(),
                example: None,
                hint: "Substring patterns must be at least one character.".into(),
            });
        }

        let results: Vec<_> = content
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(pattern))
            .map(|(idx, _)| (source_path.to_path_buf(), idx + 1))
            .collect();

        if results.is_empty() {
            return Err(Diagnostic {
                rule: "sidecar-unresolved-pattern".into(),
                severity: Severity::Error,
                node_id: None,
                file: Some(source_path.to_string_lossy().into()),
                detail: format!(
                    "Pattern '{}' did not match any line in '{}'",
                    pattern,
                    source_path.display()
                ),
                fix: "Adjust the pattern to match content in the source file.".into(),
                example: None,
                hint: "The pattern is matched as a substring against each line.".into(),
            });
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(&path).unwrap();
        write!(f, "{content}").unwrap();
        path
    }

    #[test]
    fn sidecar_not_found_returns_empty() {
        let dir = TempDir::new().unwrap();
        let src = write_file(&dir, "main.rs", "fn main() {}");
        let map = SidecarResolver::resolve(&src).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn substring_pattern_matches_line() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "app.py",
            "# this is the evidence line\ndef login(): pass\n",
        );
        write_file(
            &dir,
            "app.py.graphite",
            r#"{"anchors": {"login-flow": {"pattern": "evidence line"}}}"#,
        );
        let src = dir.path().join("app.py");
        let map = SidecarResolver::resolve(&src).unwrap();
        let locations = map.get("login-flow").unwrap();
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].1, 1);
    }

    #[test]
    fn jsonpath_pattern_resolves() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "config.json",
            r#"{"service": {"name": "auth", "port": 8080}}"#,
        );
        write_file(
            &dir,
            "config.json.graphite",
            r#"{"anchors": {"auth-service": {"pattern": "$.service.name"}}}"#,
        );
        let src = dir.path().join("config.json");
        let map = SidecarResolver::resolve(&src).unwrap();
        let locations = map.get("auth-service").unwrap();
        assert_eq!(locations.len(), 1);
        assert!(locations[0].1 >= 1);
    }

    #[test]
    fn sidecar_find_returns_correct_path() {
        let dir = TempDir::new().unwrap();
        let src = write_file(&dir, "data.json", "{}");
        write_file(&dir, "data.json.graphite", r#"{"anchors": {}}"#);
        let found = SidecarResolver::find_sidecar(&src);
        assert!(found.is_some());
        assert!(found.unwrap().exists());
    }

    #[test]
    fn sidecar_find_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        let src = write_file(&dir, "data.json", "{}");
        let found = SidecarResolver::find_sidecar(&src);
        assert!(found.is_none());
    }

    #[test]
    fn invalid_json_sidecar_returns_error() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "data.json", "{}");
        write_file(&dir, "data.json.graphite", "not valid json");
        let src = dir.path().join("data.json");
        let result = SidecarResolver::resolve(&src);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().rule, "sidecar-parse-error");
    }

    #[test]
    fn unresolvable_pattern_returns_error() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "app.rs", "fn main() {}");
        write_file(
            &dir,
            "app.rs.graphite",
            r#"{"anchors": {"my-evid": {"pattern": "nothing-like-this"}}}"#,
        );
        let src = dir.path().join("app.rs");
        let result = SidecarResolver::resolve(&src);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().rule, "sidecar-unresolved-pattern");
    }

    #[test]
    fn ambiguous_pattern_multiple_matches_error() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "app.rs", "TODO\nsome code\nTODO\n");
        write_file(
            &dir,
            "app.rs.graphite",
            r#"{"anchors": {"my-evid": {"pattern": "TODO"}}}"#,
        );
        let src = dir.path().join("app.rs");
        let result = SidecarResolver::resolve(&src);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().rule, "sidecar-ambiguous-pattern");
    }
}
