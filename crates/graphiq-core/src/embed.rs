use std::num::NonZeroU32;
use std::path::PathBuf;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::AddBos;
use llama_cpp_2::model::LlamaModel;

const GGUF_MODEL: &str = "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5-GGUF/resolve/main/nomic-embed-text-v1.5.Q8_0.gguf";
const MODEL_FILENAME: &str = "nomic-embed-text-v1.5.Q8_0.gguf";
const DIM: usize = 768;

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("llama error: {0}")]
    Llama(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no embedding text for symbol")]
    EmptyInput,
}

impl From<Box<dyn std::error::Error + Send + Sync>> for EmbedError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        EmbedError::Llama(e.to_string())
    }
}

pub struct Embedder {
    model: LlamaModel,
    backend: LlamaBackend,
}

impl Embedder {
    pub fn new(cache_dir: Option<PathBuf>) -> Result<Self, EmbedError> {
        let model_path = match cache_dir {
            Some(dir) => dir.join(MODEL_FILENAME),
            None => {
                let dir = dirs::data_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("graphiq");
                std::fs::create_dir_all(&dir).ok();
                dir.join(MODEL_FILENAME)
            }
        };

        if !model_path.exists() {
            eprintln!("  downloading GGUF model...");
            let resp = ureq::get(GGUF_MODEL)
                .call()
                .map_err(|e| EmbedError::Llama(e.to_string()))?;
            let mut reader = resp.into_body().into_reader();
            let mut file = std::fs::File::create(&model_path)?;
            std::io::copy(&mut reader, &mut file)?;
            eprintln!("  downloaded to {}", model_path.display());
        }

        let mut backend = LlamaBackend::init().map_err(|e| EmbedError::Llama(e.to_string()))?;
        backend.void_logs();

        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
            .map_err(|e| EmbedError::Llama(e.to_string()))?;

        Ok(Self { model, backend })
    }

    pub fn embed_symbol_text(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(EmbedError::EmptyInput);
        }
        let mut ctx = self.create_context()?;
        self.encode_with(&mut ctx, text)
    }

    pub fn embed_query(&self, query: &str) -> Result<Vec<f32>, EmbedError> {
        let prefixed = format!("search_query: {}", query);
        let mut ctx = self.create_context()?;
        self.encode_with(&mut ctx, &prefixed)
    }

    pub fn embed_batch(&self, texts: &[String]) -> Vec<Result<Vec<f32>, EmbedError>> {
        let mut ctx = match self.create_context() {
            Ok(c) => c,
            Err(e) => {
                let msg = e.to_string();
                return texts
                    .iter()
                    .map(|_| Err(EmbedError::Llama(msg.clone())))
                    .collect();
            }
        };
        texts
            .iter()
            .map(|text| {
                let prefixed = format!("search_document: {}", text);
                let prefixed = prefixed.trim();
                if prefixed.is_empty() {
                    return Err(EmbedError::EmptyInput);
                }
                self.encode_with(&mut ctx, prefixed)
            })
            .collect()
    }

    fn create_context(&self) -> Result<LlamaContext<'_>, EmbedError> {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(2048))
            .with_n_batch(2048)
            .with_embeddings(true);

        self.model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| EmbedError::Llama(e.to_string()))
    }

    fn encode_with(&self, ctx: &mut LlamaContext<'_>, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut tokens = self
            .model
            .str_to_token(text, AddBos::Always)
            .map_err(|e| EmbedError::Llama(e.to_string()))?;

        tokens.truncate(512);

        let mut batch =
            LlamaBatch::get_one(&tokens).map_err(|e| EmbedError::Llama(e.to_string()))?;

        ctx.encode(&mut batch)
            .map_err(|e| EmbedError::Llama(e.to_string()))?;

        let embeddings = ctx
            .embeddings_seq_ith(0)
            .map_err(|e| EmbedError::Llama(e.to_string()))?;

        let vec: Vec<f32> = embeddings.iter().take(DIM).copied().collect();
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm == 0.0 {
            return Ok(vec![0.0f32; DIM]);
        }
        Ok(vec.iter().map(|x| x / norm).collect())
    }

    pub fn dim(&self) -> usize {
        DIM
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

pub fn build_symbol_text(
    name: &str,
    signature: &Option<String>,
    doc_comment: &Option<String>,
    source: &Option<String>,
) -> String {
    let mut parts = Vec::new();
    if let Some(ref sig) = signature {
        if !sig.is_empty() {
            parts.push(sig.clone());
        }
    }
    if let Some(ref doc) = doc_comment {
        if !doc.is_empty() {
            let doc_clean: String = doc.chars().take(200).collect();
            parts.push(doc_clean);
        }
    }
    if let Some(ref src) = source {
        if !src.is_empty() {
            let src_clean: String = src.chars().take(800).collect();
            parts.push(src_clean);
        }
    }
    if parts.is_empty() {
        parts.push(name.to_string());
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b)).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn test_build_symbol_text() {
        let text = build_symbol_text(
            "authenticate",
            &Some("fn authenticate(token: &str) -> bool".into()),
            &Some("Validates the auth token".into()),
            &None,
        );
        assert!(text.contains("fn authenticate"));
        assert!(text.contains("Validates"));
    }

    #[test]
    fn test_build_symbol_text_minimal() {
        let text = build_symbol_text("main", &None, &None, &None);
        assert_eq!(text, "main");
    }
}
