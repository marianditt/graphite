use std::path::Path;

/// Graphite project configuration loaded from `graphite.yaml`.
#[derive(Clone, Debug)]
pub struct Config {
    /// Where `.node` / `.index` files live (relative to project root).
    /// Default: `"graph"`
    pub graph_dir: String,

    /// Where rendered HTML docs go (relative to project root).
    /// Default: `"docs"`
    pub output_dir: String,

    /// Directories to scan for `@graphite:evidence` anchors.
    /// Default: `["src", "tests"]`
    pub scan: Vec<String>,

    /// Base URL prefix for rendered links (e.g. `/graphite/` for GitHub Pages).
    /// Empty string means use relative links.
    pub base_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            graph_dir: "graph".into(),
            output_dir: "docs".into(),
            scan: vec!["src".into(), "tests".into()],
            base_url: String::new(),
        }
    }
}

impl Config {
    /// Load config from `graphite.yaml` in the given root directory.
    /// Returns `Ok(None)` if no file exists (caller should use defaults).
    /// Returns `Ok(Some(config))` on successful parse.
    /// Returns `Err` on parse error.
    pub fn load(root: &Path) -> Result<Option<Config>, String> {
        let config_path = root.join("graphite.yaml");
        if !config_path.exists() {
            return Ok(None);
        }

        let yaml_text = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Cannot read 'graphite.yaml': {e}"))?;

        let raw: RawConfig =
            serde_yaml::from_str(&yaml_text).map_err(|e| format!("Invalid graphite.yaml: {e}"))?;

        let mut config = Config::default();

        if let Some(gd) = raw.graph_dir {
            config.graph_dir = gd;
        }
        if let Some(od) = raw.output_dir {
            config.output_dir = od;
        }
        if let Some(scan) = raw.scan {
            config.scan = scan;
        }
        if let Some(bu) = raw.base_url {
            config.base_url = bu;
        }

        Ok(Some(config))
    }

    /// Load config and merge defaults. Always returns a valid Config.
    /// Errors from malformed YAML are surfaced — missing file is not an error.
    pub fn load_or_default(root: &Path) -> Result<Config, String> {
        match Self::load(root)? {
            Some(c) => Ok(c),
            None => Ok(Config::default()),
        }
    }
}

/// Raw deserialization target for `graphite.yaml`.
#[derive(serde::Deserialize)]
struct RawConfig {
    #[serde(default)]
    graph_dir: Option<String>,
    #[serde(default)]
    output_dir: Option<String>,
    #[serde(default)]
    scan: Option<Vec<String>>,
    #[serde(default)]
    base_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Config::load(dir.path()).unwrap().is_none());
    }

    #[test]
    fn test_full_config_parse() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("graphite.yaml"),
            r#"
graph_dir: my-graph
output_dir: site
scan:
  - src
  - tests
  - graphite-core/src
"#,
        )
        .unwrap();

        let config = Config::load(dir.path()).unwrap().unwrap();
        assert_eq!(config.graph_dir, "my-graph");
        assert_eq!(config.output_dir, "site");
        assert_eq!(config.scan, vec!["src", "tests", "graphite-core/src"]);
    }

    #[test]
    fn test_partial_config_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("graphite.yaml"),
            r#"
graph_dir: custom
"#,
        )
        .unwrap();

        let config = Config::load(dir.path()).unwrap().unwrap();
        assert_eq!(config.graph_dir, "custom");
        assert_eq!(config.output_dir, "docs"); // default
        assert_eq!(config.scan, vec!["src", "tests"]); // default
    }

    #[test]
    fn test_invalid_yaml_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("graphite.yaml"), "invalid: [").unwrap();
        assert!(Config::load(dir.path()).is_err());
    }

    #[test]
    fn test_load_or_default_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load_or_default(dir.path()).unwrap();
        assert_eq!(config.graph_dir, "graph");
        assert_eq!(config.output_dir, "docs");
    }
}
