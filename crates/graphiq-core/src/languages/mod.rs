//! Per-language Tree-sitter chunkers.
//!
//! Each sub-module implements [`LanguageChunker`](crate::chunker::LanguageChunker)
//! for a specific language, handling its syntax for symbol extraction,
//! import resolution, structural relations, and doc comment parsing.
//!
//! Supported: c, cpp, css, go, html, java, json, python, ruby, rust, toml,
//! typescript, yaml. TypeScript chunker also handles TSX/JSX/JavaScript.

pub mod c;
pub mod cpp;
pub mod css;
pub mod go;
pub mod html;
pub mod java;
pub mod json;
pub mod python;
pub mod ruby;
pub mod rust;
pub mod toml;
pub mod typescript;
pub mod yaml;
