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

pub fn walk_project(root: &Path) -> impl Iterator<Item = PathBuf> {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .add_custom_ignore_filename(".graphiqignore");

    builder.filter_entry(|entry| {
        let name = entry.file_name().to_string_lossy();
        if name == ".git"
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
}
