//! File discovery, language detection, and content hashing.
//!
//! Walks a project directory respecting `.gitignore`, detects languages from
//! file extensions, and computes SHA-256 content hashes for incremental
//! reindexing. Supports 36+ languages with full parsing for 16.
//!
//! Key functions: [`walk_project`] (file iterator), [`detect_language`]
//! (extension-based detection), [`content_hash`] (SHA-256).

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum Language {
    TypeScript,
    TSX,
    JavaScript,
    JSX,
    Rust,
    Python,
    Go,
    Java,
    C,
    Cpp,
    CMake,
    Qml,
    Meson,
    Ruby,
    Markdown,
    Json,
    Yaml,
    Toml,
    Html,
    Css,
    Scss,
    Shell,
    Sql,
    Dockerfile,
    Makefile,
    Kotlin,
    Swift,
    CSharp,
    Php,
    Lua,
    Dart,
    Scala,
    Haskell,
    Elixir,
    Zig,
    Xml,
    GraphQL,
    Protobuf,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "ts" => Language::TypeScript,
            "tsx" => Language::TSX,
            "js" | "mjs" | "cjs" => Language::JavaScript,
            "jsx" => Language::JSX,
            "rs" => Language::Rust,
            "py" | "pyi" => Language::Python,
            "go" => Language::Go,
            "java" => Language::Java,
            "c" | "h" => Language::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Language::Cpp,
            "qml" => Language::Qml,
            "rb" => Language::Ruby,
            "md" | "mdx" => Language::Markdown,
            "json" | "jsonc" => Language::Json,
            "yml" | "yaml" => Language::Yaml,
            "toml" => Language::Toml,
            "html" | "htm" => Language::Html,
            "css" | "less" => Language::Css,
            "scss" | "sass" => Language::Scss,
            "sh" | "bash" | "zsh" | "fish" => Language::Shell,
            "sql" => Language::Sql,
            "kt" | "kts" => Language::Kotlin,
            "swift" => Language::Swift,
            "cs" => Language::CSharp,
            "php" => Language::Php,
            "lua" => Language::Lua,
            "dart" => Language::Dart,
            "scala" | "sc" => Language::Scala,
            "hs" => Language::Haskell,
            "ex" | "exs" => Language::Elixir,
            "zig" => Language::Zig,
            "xml" | "svg" | "xsl" | "xslt" => Language::Xml,
            "graphql" | "gql" => Language::GraphQL,
            "proto" => Language::Protobuf,
            _ => Language::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::TypeScript => "typescript",
            Language::TSX => "tsx",
            Language::JavaScript => "javascript",
            Language::JSX => "jsx",
            Language::Rust => "rust",
            Language::Python => "python",
            Language::Go => "go",
            Language::Java => "java",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::CMake => "cmake",
            Language::Qml => "qml",
            Language::Meson => "meson",
            Language::Ruby => "ruby",
            Language::Markdown => "markdown",
            Language::Json => "json",
            Language::Yaml => "yaml",
            Language::Toml => "toml",
            Language::Html => "html",
            Language::Css => "css",
            Language::Scss => "scss",
            Language::Shell => "shell",
            Language::Sql => "sql",
            Language::Dockerfile => "dockerfile",
            Language::Makefile => "makefile",
            Language::Kotlin => "kotlin",
            Language::Swift => "swift",
            Language::CSharp => "csharp",
            Language::Php => "php",
            Language::Lua => "lua",
            Language::Dart => "dart",
            Language::Scala => "scala",
            Language::Haskell => "haskell",
            Language::Elixir => "elixir",
            Language::Zig => "zig",
            Language::Xml => "xml",
            Language::GraphQL => "graphql",
            Language::Protobuf => "protobuf",
            Language::Unknown => "unknown",
        }
    }

    pub fn supported(&self) -> bool {
        !matches!(self, Language::Unknown)
    }
}

pub fn detect_language(path: &Path) -> Language {
    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match fname {
        "CMakeLists.txt" => return Language::CMake,
        "meson.build" | "meson_options.txt" => return Language::Meson,
        "Dockerfile" | "dockerfile" => return Language::Dockerfile,
        "Makefile" | "makefile" | "GNUmakefile" => return Language::Makefile,
        _ => {}
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext == "cmake" {
            return Language::CMake;
        }
        return Language::from_extension(ext);
    }
    Language::Unknown
}

pub fn content_hash(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// Maximum size at which a data-format file (JSON/YAML/TOML) is still symbol-extracted.
/// Files larger than this are treated as opaque blobs: file-tracked for freshness,
/// but never parsed into symbols. This prevents generated data — dependency
/// lockfiles, benchmark dumps, vendored snapshots — from dominating the symbol
/// graph with thousands of low-value keys.
pub const MAX_DATA_FILE_SYMBOL_BYTES: u64 = 256 * 1024;

/// Dependency lockfiles and generated vendored-data filenames. These carry no
/// code-intelligence value and must never be symbol-extracted regardless of size.
fn is_lockfile_name(name: &str) -> bool {
    matches!(
        name,
        "package-lock.json"
            | "npm-shrinkwrap.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "pnpm-lock.yml"
            | "composer.lock"
            | "Gemfile.lock"
            | "Cargo.lock"
            | "poetry.lock"
            | "Pipfile.lock"
            | "uv.lock"
            | "flake.lock"
            | "deno.lock"
            | "go.sum"
            | "bsb.lock"
            | "esbuild.lock"
            | "terraform.lock.hcl"
            | "gradle.lockfile"
            | "packages.lock.json"
            | "mix.lock"
            | "Podfile.lock"
            | "Cartfile.resolved"
    )
}

/// Returns true for files that should be **file-tracked** (so freshness/staleness
/// still works) but **never symbol-extracted**.
///
/// Covers two cases:
/// 1. Dependency lockfiles / generated vendored data (by name or `*-lock`/`*.lock`
///    shape) — e.g. `package-lock.json`, `Cargo.lock`, `pnpm-lock.yaml`. A single
///    `package-lock.json` can otherwise produce thousands of junk `Constant`
///    symbols (one per JSON key) and silently dominate search results and the
///    codebase briefing.
/// 2. Oversized data-format files (JSON/YAML/TOML above `MAX_DATA_FILE_SYMBOL_BYTES`)
///    — generated data dumps that have no useful per-symbol structure.
///
/// Source code (`.rs`, `.ts`, `.py`, ...) and small config files (`tsconfig.json`,
/// `package.json`) are unaffected and still get full symbol extraction.
pub fn is_data_file(path: &Path, size_bytes: u64) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if is_lockfile_name(name) {
        return true;
    }
    // Lockfile-shaped names not in the literal list: *-lock.json, *.lock.json, *.lock
    if name.ends_with("-lock.json")
        || name.ends_with(".lock.json")
        || name.ends_with(".lock")
        || name.ends_with(".lockfile")
    {
        return true;
    }
    // Oversized data formats: a multi-megabyte JSON/YAML/TOML blob is generated
    // data, not hand-written code, and has no useful per-symbol structure.
    if size_bytes > MAX_DATA_FILE_SYMBOL_BYTES {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(
            ext.to_lowercase().as_str(),
            "json" | "jsonc" | "yaml" | "yml" | "toml"
        ) {
            return true;
        }
    }
    false
}

pub fn walk_project(root: &Path) -> impl Iterator<Item = PathBuf> {
    let root_owned = root.to_path_buf();
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .add_custom_ignore_filename(".graphiqignore");

    builder.filter_entry(move |entry| {
        let name = entry.file_name().to_string_lossy();
        if name == ".git"
            || name == ".github"
            || name == "node_modules"
            || name == "target"
            || name == ".graphiq"
            || name == "dist"
            || name == "build"
            || name == "__pycache__"
            || name == ".venv"
            || name == "vendor"
            || name == ".next"
            || name == ".nuxt"
            || name == "coverage"
            || name == ".sqmd"
        {
            return false;
        }
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            if entry.path().join(".git").exists() && entry.path() != root_owned {
                return false;
            }
        }
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            let path_str = entry.path().to_string_lossy();
            if path_str.contains("-bundle.")
                || path_str.contains("-bundle/")
                || path_str.contains(".min.js")
                || path_str.contains(".min.css")
            {
                return false;
            }
            let lang = detect_language(entry.path());
            return lang.supported();
        }
        true
    });

    builder.build().filter_map(|entry| {
        let entry = entry.ok()?;
        if entry.file_type()?.is_file() {
            Some(entry.path().to_path_buf())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(
            detect_language(Path::new("src/main.ts")),
            Language::TypeScript
        );
        assert_eq!(detect_language(Path::new("src/App.tsx")), Language::TSX);
        assert_eq!(detect_language(Path::new("src/main.rs")), Language::Rust);
        assert_eq!(detect_language(Path::new("src/main.py")), Language::Python);
        assert_eq!(detect_language(Path::new("Cargo.toml")), Language::Toml);
        assert_eq!(detect_language(Path::new("data.xyz")), Language::Unknown);
        assert_eq!(
            detect_language(Path::new("Dockerfile")),
            Language::Dockerfile
        );
        assert_eq!(detect_language(Path::new("Makefile")), Language::Makefile);
        assert_eq!(detect_language(Path::new("deploy.sh")), Language::Shell);
        assert_eq!(
            detect_language(Path::new("schema.graphql")),
            Language::GraphQL
        );
        assert_eq!(detect_language(Path::new("api.proto")), Language::Protobuf);
    }

    #[test]
    fn test_content_hash_deterministic() {
        let h1 = content_hash(b"hello world");
        let h2 = content_hash(b"hello world");
        let h3 = content_hash(b"hello worle");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_is_data_file_excludes_lockfiles() {
        // Lockfiles are excluded at any size (by name).
        for (name, size) in [
            ("package-lock.json", 10u64),
            ("Cargo.lock", 10),
            ("pnpm-lock.yaml", 10),
            ("yarn.lock", 10),
            ("go.sum", 10),
            ("packages.lock.json", 10),
        ] {
            assert!(
                is_data_file(Path::new(name), size),
                "{name} should be a data file"
            );
        }

        // Lockfile-shaped names not in the literal list.
        assert!(is_data_file(Path::new("some-pkg-lock.json"), 10));
        assert!(is_data_file(Path::new("deps.lock.json"), 10));
        assert!(is_data_file(Path::new("whatever.lock"), 10));

        // Path-prefixed lockfiles.
        assert!(is_data_file(
            Path::new("apps/desktop/package-lock.json"),
            10
        ));
    }

    #[test]
    fn test_is_data_file_keeps_code_and_small_config() {
        // Real source code is never a data file.
        assert!(!is_data_file(Path::new("src/main.rs"), 10));
        assert!(!is_data_file(Path::new("src/index.ts"), 10));
        assert!(!is_data_file(Path::new("lib/app.py"), 10));

        // Small hand-written config is still parsed.
        assert!(!is_data_file(Path::new("tsconfig.json"), 500));
        assert!(!is_data_file(Path::new("package.json"), 2_000));
        assert!(!is_data_file(Path::new("Cargo.toml"), 1_000));
    }

    #[test]
    fn test_is_data_file_excludes_oversized_data() {
        // A multi-megabyte JSON/YAML/TOML blob is treated as data even without a
        // lockfile name.
        assert!(is_data_file(
            Path::new("benches/ndcg-50-tokio.json"),
            5_000_000
        ));
        assert!(is_data_file(Path::new("snapshot.yaml"), 2_000_000));
        assert!(is_data_file(Path::new("generated.toml"), 1_000_000));

        // Same path at a small size is still parsed (it may be real config).
        assert!(!is_data_file(
            Path::new("benches/ndcg-50-tokio.json"),
            1_000
        ));

        // Oversized source code is never treated as data — code is always parsed.
        assert!(!is_data_file(Path::new("huge.rs"), 5_000_000));
    }
}
