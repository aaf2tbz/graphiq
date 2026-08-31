//! Cross-platform GPU compute acceleration for graphiq indexing and search.
//!
//! Delegates data-parallel cruncher operations (TF normalization, IDF
//! computation, bridging potential, and large search term batches) from CPU
//! RAM to GPU memory via wgpu compute shaders — Metal on macOS, Vulkan on
//! Linux, DX12 on Windows.
//!
//! Falls back to rayon CPU multithreading when no GPU is available.
//! The GPU path reduces main-memory pressure by keeping intermediate
//! arrays in VRAM during the heaviest cruncher phases.

use std::collections::HashMap;

use rayon::prelude::*;

/// Version for the compact representation retained by [`GpuIndexData`].
/// Increment this when the packed layout changes in an incompatible way.
pub const GPU_INDEX_DATA_VERSION: u32 = 1;

const DEFAULT_GPU_MIN_INDEX_SYMBOLS: usize = 32;
const DEFAULT_GPU_MIN_INDEX_TERMS: usize = 128;
const DEFAULT_GPU_MIN_SEARCH_CANDIDATES: usize = 128;
const DEFAULT_GPU_MIN_SEARCH_WORK_ITEMS: usize = 32768;

#[cfg(feature = "gpu")]
static GPU_SEARCH_DISPATCHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub fn gpu_trace_enabled() -> bool {
    env_flag("GRAPHIQ_GPU_TRACE")
}

pub fn reset_gpu_search_dispatch_count() {
    #[cfg(feature = "gpu")]
    GPU_SEARCH_DISPATCHES.store(0, std::sync::atomic::Ordering::Relaxed);
}

pub fn gpu_search_dispatch_count() -> u64 {
    #[cfg(feature = "gpu")]
    return GPU_SEARCH_DISPATCHES.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(feature = "gpu"))]
    0
}

/// Minimum number of symbols before index crunching considers the GPU.
pub fn gpu_min_index_symbols() -> usize {
    env_usize(
        "GRAPHIQ_GPU_MIN_INDEX_SYMBOLS",
        DEFAULT_GPU_MIN_INDEX_SYMBOLS,
    )
}

/// Minimum number of packed term entries before index crunching considers the
/// GPU. This prevents paying GPU setup costs for tiny indexes.
pub fn gpu_min_index_terms() -> usize {
    env_usize("GRAPHIQ_GPU_MIN_INDEX_TERMS", DEFAULT_GPU_MIN_INDEX_TERMS)
}

/// Minimum number of candidates before a search score batch considers the GPU.
pub fn gpu_min_search_candidates() -> usize {
    env_usize(
        "GRAPHIQ_GPU_MIN_SEARCH_CANDIDATES",
        DEFAULT_GPU_MIN_SEARCH_CANDIDATES,
    )
}

/// Minimum approximate query/candidate work items before a search score batch
/// considers the GPU.
pub fn gpu_min_search_work_items() -> usize {
    env_usize(
        "GRAPHIQ_GPU_MIN_SEARCH_WORK_ITEMS",
        DEFAULT_GPU_MIN_SEARCH_WORK_ITEMS,
    )
}

pub fn should_use_gpu_for_index(n_symbols: usize, n_term_entries: usize) -> bool {
    n_symbols >= gpu_min_index_symbols() && n_term_entries >= gpu_min_index_terms()
}

pub fn should_use_gpu_for_search(
    n_candidates: usize,
    n_query_probes: usize,
    n_candidate_terms: usize,
) -> bool {
    n_candidates >= gpu_min_search_candidates()
        && n_candidate_terms.saturating_mul(n_query_probes.max(1)) >= gpu_min_search_work_items()
}

/// The adapter selected by wgpu. Kept independent from wgpu's types so status
/// output and serialized/index-facing APIs do not expose backend internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuDeviceInfo {
    pub backend: String,
    pub name: String,
    pub device_type: String,
    pub vendor: u32,
    pub device: u32,
}

impl std::fmt::Display for GpuDeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}, {})", self.backend, self.name, self.device_type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuStatus {
    /// The binary was built without the optional GPU feature.
    Disabled,
    /// A device was initialized and is available for dispatch.
    Available(GpuDeviceInfo),
    /// GPU support was compiled in but initialization failed or was disabled.
    Unavailable(String),
}

/// Compact, serializable data needed to score query terms against symbols on
/// the GPU. The representation contains only the final top-term sets used by
/// search, rather than the larger raw indexing buffers.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct GpuIndexData {
    pub version: u32,
    #[serde(default)]
    pub fingerprint: u64,
    pub terms: Vec<String>,
    pub term_to_id: HashMap<String, u32>,
    pub term_idf: Vec<f32>,
    pub term_ids: Vec<u32>,
    pub symbol_offsets: Vec<[u32; 2]>,
    pub term_weights: Vec<f32>,
}

impl GpuIndexData {
    /// Build the compact representation from the finalized CPU term sets.
    /// This method is only called by GPU-enabled index builds; CPU-only builds
    /// leave the optional representation absent.
    pub fn from_term_sets(
        term_sets: &[crate::cruncher::TermSet],
        global_idf: &HashMap<String, f64>,
    ) -> Self {
        let mut terms: Vec<String> = term_sets
            .iter()
            .flat_map(|term_set| term_set.terms.keys().cloned())
            .collect();
        terms.sort_unstable();
        terms.dedup();

        let term_to_id: HashMap<String, u32> = terms
            .iter()
            .enumerate()
            .map(|(id, term)| (term.clone(), id as u32))
            .collect();
        let term_idf: Vec<f32> = terms
            .iter()
            .map(|term| global_idf.get(term).copied().unwrap_or(1.0) as f32)
            .collect();

        let mut term_ids = Vec::new();
        let mut term_weights = Vec::new();
        let mut symbol_offsets = Vec::with_capacity(term_sets.len());
        for term_set in term_sets {
            let start = term_ids.len() as u32;
            let mut entries: Vec<(&String, &f64)> = term_set.terms.iter().collect();
            entries.sort_unstable_by_key(|(term, _)| term.as_str());
            for (term, weight) in entries {
                if let Some(&id) = term_to_id.get(term) {
                    term_ids.push(id);
                    term_weights.push(*weight as f32);
                }
            }
            symbol_offsets.push([start, term_ids.len() as u32 - start]);
        }

        let mut data = Self {
            version: GPU_INDEX_DATA_VERSION,
            fingerprint: 0,
            terms,
            term_to_id,
            term_idf,
            term_ids,
            symbol_offsets,
            term_weights,
        };
        data.fingerprint = data.compute_fingerprint();
        data
    }

    pub fn is_compatible(&self) -> bool {
        if self.version != GPU_INDEX_DATA_VERSION
            || self.terms.len() != self.term_idf.len()
            || self.terms.len() != self.term_to_id.len()
            || self.term_ids.len() != self.term_weights.len()
        {
            return false;
        }
        if self
            .terms
            .iter()
            .enumerate()
            .any(|(id, term)| self.term_to_id.get(term) != Some(&(id as u32)))
        {
            return false;
        }

        let mut expected_start = 0u32;
        for [start, len] in &self.symbol_offsets {
            if *start != expected_start {
                return false;
            }
            expected_start = expected_start.saturating_add(*len);
        }
        expected_start as usize == self.term_ids.len()
    }

    pub fn fingerprint(&self) -> u64 {
        if self.fingerprint != 0 {
            self.fingerprint
        } else {
            self.compute_fingerprint()
        }
    }

    fn compute_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.version.hash(&mut hasher);
        self.symbol_offsets.hash(&mut hasher);
        self.term_ids.hash(&mut hasher);
        for weight in &self.term_weights {
            weight.to_bits().hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn packed_term_count(&self) -> usize {
        self.term_ids.len()
    }

    /// Convert query variants into sparse compact-term probes. A probe weight
    /// is the same substring/exact-match ratio used by `term_match_score()`.
    pub fn pack_query(&self, query_terms: &[crate::cruncher::QueryTerm]) -> PackedGpuQuery {
        let mut query_idfs = Vec::with_capacity(query_terms.len());
        let mut query_offsets = Vec::with_capacity(query_terms.len());
        let mut probe_ids = Vec::new();
        let mut probe_weights = Vec::new();

        for query_term in query_terms {
            query_idfs.push(query_term.idf as f32);
            let start = probe_ids.len() as u32;
            let mut ratios: HashMap<u32, f32> = HashMap::new();

            for variant in &query_term.variants {
                for (id, term) in self.terms.iter().enumerate() {
                    let ratio = if term == variant {
                        Some(1.0)
                    } else if term.contains(variant) || variant.contains(term.as_str()) {
                        Some(
                            term.len().min(variant.len()) as f32
                                / term.len().max(variant.len()) as f32,
                        )
                    } else {
                        None
                    };
                    if let Some(ratio) = ratio {
                        let entry = ratios.entry(id as u32).or_default();
                        *entry = (*entry).max(ratio);
                    }
                }
            }

            let mut probes: Vec<(u32, f32)> = ratios.into_iter().collect();
            probes.sort_unstable_by_key(|(id, _)| *id);
            for (id, ratio) in probes {
                probe_ids.push(id);
                probe_weights.push(ratio);
            }
            query_offsets.push([start, probe_ids.len() as u32 - start]);
        }

        PackedGpuQuery {
            query_idfs,
            query_offsets,
            probe_ids,
            probe_weights,
        }
    }
}

pub struct PackedGpuQuery {
    pub query_idfs: Vec<f32>,
    pub query_offsets: Vec<[u32; 2]>,
    pub probe_ids: Vec<u32>,
    pub probe_weights: Vec<f32>,
}

#[derive(Debug)]
pub struct GpuSearchResult {
    pub candidate_indices: Vec<usize>,
    pub scores: Vec<f32>,
    pub matched: Vec<u32>,
    pub elapsed_ms: u64,
    pub device: GpuDeviceInfo,
}

// ---------------------------------------------------------------------------
// WGSL compute shaders
// ---------------------------------------------------------------------------

#[cfg(feature = "gpu")]
const TF_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> counts: array<f32>;
@group(0) @binding(1) var<storage, read>     offsets: array<vec2<u32>>;
@group(0) @binding(2) var<uniform>          params: vec4<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let sym = gid.x;
    if (sym >= params.x) { return; }
    let s = offsets[sym].x;
    let len = offsets[sym].y;
    var total = 0.0;
    for (var i = 0u; i < len; i = i + 1u) {
        total += counts[s + i];
    }
    if (total > 0.0) {
        for (var i = 0u; i < len; i = i + 1u) {
            counts[s + i] /= total;
        }
    }
}
"#;

#[cfg(feature = "gpu")]
const IDF_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read>      doc_freq: array<f32>;
@group(0) @binding(1) var<storage, read_write> idf_out:  array<f32>;
@group(0) @binding(2) var<uniform>             params: vec4<u32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.x) { return; }
    let n = f32(params.y);
    idf_out[i] = log(1.0 + n / (doc_freq[i] + 1.0));
}
"#;

#[cfg(feature = "gpu")]
const BRIDGE_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read>       sym_offsets: array<vec2<u32>>;
@group(0) @binding(1) var<storage, read>       term_ids:   array<u32>;
@group(0) @binding(2) var<storage, read>       edge_data:  array<u32>;
@group(0) @binding(3) var<storage, read>       edge_off:   array<vec2<u32>>;
@group(0) @binding(4) var<storage, read_write> bridging:   array<f32>;
@group(0) @binding(5) var<uniform>             params:     vec4<u32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let sym = gid.x;
    if (sym >= params.x) { return; }

    let my_s = sym_offsets[sym].x;
    let my_l = sym_offsets[sym].y;

    var own: array<u32, 64>;
    var own_n = 0u;
    for (var i = 0u; i < min(my_l, 64u); i = i + 1u) {
        own[i] = term_ids[my_s + i];
        own_n += 1u;
    }

    let es = edge_off[sym].x;
    let el = edge_off[sym].y;

    var novel = 0.0;
    var total = 0.0;
    for (var e = 0u; e < min(el, 20u); e = e + 1u) {
        let nb = edge_data[es + e];
        let ns = sym_offsets[nb].x;
        let nl = sym_offsets[nb].y;
        for (var t = 0u; t < min(nl, 30u); t = t + 1u) {
            let tid = term_ids[ns + t];
            total += 1.0;
            var found = false;
            for (var o = 0u; o < own_n; o = o + 1u) {
                if (own[o] == tid) { found = true; break; }
            }
            if (!found) { novel += 1.0; }
        }
    }

    let novelty = select(novel / total, 0.0, total == 0.0);
    let boost = log(2.0 + f32(el)) * 0.3;
    bridging[sym] = novelty * (1.0 + boost);
}
"#;

#[cfg(feature = "gpu")]
const TERM_MATCH_SHADER: &str = r#"
struct QueryProbe {
    term_id: u32,
    weight: f32,
};

struct MatchResult {
    score: f32,
    matched: u32,
};

@group(0) @binding(0) var<storage, read>     q_idfs:    array<f32>;
@group(0) @binding(1) var<storage, read>     q_offsets: array<vec2<u32>>;
@group(0) @binding(2) var<storage, read>     q_probes:  array<QueryProbe>;
@group(0) @binding(3) var<storage, read>     s_offsets: array<vec2<u32>>;
@group(0) @binding(4) var<storage, read>     s_terms:   array<u32>;
@group(0) @binding(5) var<storage, read>     s_weights: array<f32>;
@group(0) @binding(6) var<storage, read>     candidates: array<u32>;
@group(0) @binding(7) var<storage, read_write> results: array<MatchResult>;
@group(0) @binding(8) var<uniform>               params: vec4<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let candidate_index = gid.x;
    if (candidate_index >= params.x) { return; }

    let symbol = candidates[candidate_index];
    let symbol_offset = s_offsets[symbol];
    var score = 0.0;
    var matched = 0u;

    for (var q = 0u; q < params.y; q = q + 1u) {
        let query_offset = q_offsets[q];
        var best = 0.0;
        for (var t = 0u; t < symbol_offset.y; t = t + 1u) {
            let symbol_term = s_terms[symbol_offset.x + t];
            let symbol_weight = s_weights[symbol_offset.x + t];
            for (var p = 0u; p < query_offset.y; p = p + 1u) {
                let probe = q_probes[query_offset.x + p];
                if (symbol_term == probe.term_id) {
                    best = max(best, symbol_weight * probe.weight);
                }
            }
        }
        if (best > 0.0) {
            matched = matched + 1u;
            score = score + q_idfs[q] * best;
        }
    }

    results[candidate_index].score = score;
    results[candidate_index].matched = matched;
}
"#;

#[cfg(feature = "gpu")]
const SCORE_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read>       q_ids:    array<u32>;
@group(0) @binding(1) var<storage, read>       q_idfs:   array<f32>;
@group(0) @binding(2) var<storage, read>       s_off:    array<vec2<u32>>;
@group(0) @binding(3) var<storage, read>       s_tids:   array<u32>;
@group(0) @binding(4) var<storage, read>       s_tfs:    array<f32>;
@group(0) @binding(5) var<storage, read>       cands:    array<u32>;
@group(0) @binding(6) var<storage, read_write> scores:   array<f32>;
@group(0) @binding(7) var<uniform>             params:   vec4<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let ci = gid.x;
    if (ci >= params.x) { return; }
    let sym = cands[ci];
    let ss = s_off[sym].x;
    let sl = s_off[sym].y;
    var score = 0.0;
    for (var q = 0u; q < params.y; q = q + 1u) {
        let qid = q_ids[q];
        let qi  = q_idfs[q];
        for (var t = 0u; t < sl; t = t + 1u) {
            if (s_tids[ss + t] == qid) {
                score += qi * s_tfs[ss + t];
            }
        }
    }
    scores[ci] = score;
}
"#;

// ---------------------------------------------------------------------------
// GpuContext — wgpu device/queue + cached pipelines
// ---------------------------------------------------------------------------

#[cfg(feature = "gpu")]
mod wgpu_backend {
    use std::sync::{Arc, Mutex};

    use super::GpuDeviceInfo;

    struct ResidentIndex {
        symbol_offsets: wgpu::Buffer,
        term_ids: wgpu::Buffer,
        term_weights: wgpu::Buffer,
    }

    pub struct GpuContext {
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        info: GpuDeviceInfo,
        resident_index: Mutex<Option<(u64, Arc<ResidentIndex>)>>,
        tf_pipe: wgpu::ComputePipeline,
        idf_pipe: wgpu::ComputePipeline,
        bridge_pipe: wgpu::ComputePipeline,
        term_match_pipe: wgpu::ComputePipeline,
        score_pipe: wgpu::ComputePipeline,
    }

    impl GpuContext {
        pub fn new() -> Option<Self> {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(Self::init_inner));
            result.ok().flatten()
        }

        fn init_inner() -> Option<Self> {
            let backends = if cfg!(target_os = "macos") {
                // Do not accidentally select a portability or GL adapter on
                // macOS. Metal is part of the operating system and is the
                // supported native backend for Apple Silicon.
                wgpu::Backends::METAL
            } else {
                wgpu::Backends::all()
            };
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends,
                ..Default::default()
            });

            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                }))?;

            let adapter_info = adapter.get_info();
            let info = GpuDeviceInfo {
                backend: format!("{:?}", adapter_info.backend),
                name: adapter_info.name,
                device_type: format!("{:?}", adapter_info.device_type),
                vendor: adapter_info.vendor,
                device: adapter_info.device,
            };

            let adapter_limits = adapter.limits();
            if adapter_limits.max_storage_buffers_per_shader_stage < 8 {
                return None;
            }
            let required_limits = wgpu::Limits {
                max_storage_buffers_per_shader_stage: 8,
                ..wgpu::Limits::downlevel_defaults()
            };

            let (device, queue) = pollster::block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("graphiq-gpu"),
                    required_features: wgpu::Features::empty(),
                    required_limits,
                    ..Default::default()
                },
                None,
            ))
            .ok()?;

            let device = Arc::new(device);
            let queue = Arc::new(queue);

            let tf_pipe = Self::make_pipeline(&device, super::TF_SHADER, "tf");
            let idf_pipe = Self::make_pipeline(&device, super::IDF_SHADER, "idf");
            let bridge_pipe = Self::make_pipeline(&device, super::BRIDGE_SHADER, "bridge");
            let term_match_pipe =
                Self::make_pipeline(&device, super::TERM_MATCH_SHADER, "term-match");
            let score_pipe = Self::make_pipeline(&device, super::SCORE_SHADER, "score");

            Some(Self {
                device,
                queue,
                info,
                resident_index: Mutex::new(None),
                tf_pipe,
                idf_pipe,
                bridge_pipe,
                term_match_pipe,
                score_pipe,
            })
        }

        pub fn info(&self) -> &GpuDeviceInfo {
            &self.info
        }

        fn resident_index(&self, data: &super::GpuIndexData) -> Arc<ResidentIndex> {
            let fingerprint = data.fingerprint();
            if let Ok(guard) = self.resident_index.lock() {
                if let Some((cached_fingerprint, resident)) = guard.as_ref() {
                    if *cached_fingerprint == fingerprint {
                        return resident.clone();
                    }
                }
            }

            let resident = Arc::new(ResidentIndex {
                symbol_offsets: self.storage_init(
                    &super::u32pairs_to_bytes(&data.symbol_offsets),
                    "tm-s-offsets",
                    false,
                ),
                term_ids: self.storage_init(
                    &super::u32_to_bytes(&data.term_ids),
                    "tm-s-terms",
                    false,
                ),
                term_weights: self.storage_init(
                    &super::f32_to_bytes(&data.term_weights),
                    "tm-s-weights",
                    false,
                ),
            });
            if let Ok(mut guard) = self.resident_index.lock() {
                *guard = Some((fingerprint, resident.clone()));
            }
            resident
        }

        fn make_pipeline(device: &wgpu::Device, src: &str, label: &str) -> wgpu::ComputePipeline {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        }

        fn storage_init(&self, bytes: &[u8], label: &str, readback: bool) -> wgpu::Buffer {
            use wgpu::util::DeviceExt;
            let mut usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
            if readback {
                usage |= wgpu::BufferUsages::COPY_SRC;
            }
            let contents: &[u8] = if bytes.is_empty() {
                &[0, 0, 0, 0]
            } else {
                bytes
            };
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage,
                })
        }

        fn uniform_init(&self, bytes: &[u8], label: &str) -> wgpu::Buffer {
            use wgpu::util::DeviceExt;
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: wgpu::BufferUsages::UNIFORM,
                })
        }

        fn staging(&self, size: u64) -> wgpu::Buffer {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("staging"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        }

        fn readback_bytes(&self, buf: &wgpu::Buffer, byte_len: u64) -> Option<Vec<u8>> {
            if byte_len == 0 {
                return Some(Vec::new());
            }
            let staging = self.staging(byte_len);

            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("readback-enc"),
                });
            enc.copy_buffer_to_buffer(buf, 0, &staging, 0, byte_len);
            self.queue.submit(std::iter::once(enc.finish()));

            let slice = staging.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            self.device.poll(wgpu::Maintain::Wait);
            if rx.recv().ok()?.is_err() {
                return None;
            }

            let view = slice.get_mapped_range();
            let out = view.to_vec();
            drop(view);
            staging.unmap();
            Some(out)
        }

        fn readback_f32(&self, buf: &wgpu::Buffer, count: usize) -> Option<Vec<f32>> {
            let bytes = self.readback_bytes(buf, (count * 4) as u64)?;
            Some(super::bytes_to_f32(&bytes))
        }

        fn readback_matches(
            &self,
            buf: &wgpu::Buffer,
            count: usize,
        ) -> Option<(Vec<f32>, Vec<u32>)> {
            let bytes = self.readback_bytes(buf, (count * 8) as u64)?;
            let mut scores = Vec::with_capacity(count);
            let mut matched = Vec::with_capacity(count);
            for chunk in bytes.chunks_exact(8) {
                scores.push(f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
                matched.push(u32::from_ne_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
            }
            Some((scores, matched))
        }

        // -- public dispatch ------------------------------------------------

        pub fn dispatch_tf(
            &self,
            counts: &[f32],
            offsets: &[[u32; 2]],
            n_symbols: u32,
        ) -> Option<Vec<f32>> {
            let counts_b = self.storage_init(&super::f32_to_bytes(counts), "tf-c", true);
            let offsets_b = self.storage_init(&super::u32pairs_to_bytes(offsets), "tf-o", false);
            let params_bytes: Vec<u8> = [n_symbols, 0u32, 0u32, 0u32]
                .iter()
                .flat_map(|v| v.to_ne_bytes())
                .collect();
            let params_b = self.uniform_init(&params_bytes, "tf-p");

            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tf-bg"),
                layout: &self.tf_pipe.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: counts_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: offsets_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params_b.as_entire_binding(),
                    },
                ],
            });

            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("tf-enc"),
                });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("tf-pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.tf_pipe);
                pass.set_bind_group(0, &bg, &[]);
                pass.dispatch_workgroups(n_symbols.div_ceil(64), 1, 1);
            }
            self.queue.submit(std::iter::once(enc.finish()));
            self.readback_f32(&counts_b, counts.len())
        }

        pub fn dispatch_idf(&self, doc_freq: &[f32], n_symbols: u32) -> Option<Vec<f32>> {
            let n_terms = doc_freq.len() as u32;
            let df_b = self.storage_init(&super::f32_to_bytes(doc_freq), "idf-df", false);
            let out_b = self.storage_init(&super::f32_to_bytes(doc_freq), "idf-out", true);
            let params_bytes: Vec<u8> = [n_terms, n_symbols, 0u32, 0u32]
                .iter()
                .flat_map(|v| v.to_ne_bytes())
                .collect();
            let params_b = self.uniform_init(&params_bytes, "idf-p");

            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("idf-bg"),
                layout: &self.idf_pipe.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: df_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: out_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params_b.as_entire_binding(),
                    },
                ],
            });

            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("idf-enc"),
                });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("idf-pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.idf_pipe);
                pass.set_bind_group(0, &bg, &[]);
                pass.dispatch_workgroups(n_terms.div_ceil(256), 1, 1);
            }
            self.queue.submit(std::iter::once(enc.finish()));
            self.readback_f32(&out_b, doc_freq.len())
        }

        pub fn dispatch_bridge(
            &self,
            sym_offsets: &[[u32; 2]],
            term_ids: &[u32],
            edge_data: &[u32],
            edge_off: &[[u32; 2]],
        ) -> Option<Vec<f32>> {
            let n = sym_offsets.len() as u32;
            let so_b = self.storage_init(&super::u32pairs_to_bytes(sym_offsets), "br-so", false);
            let ti_b = self.storage_init(&super::u32_to_bytes(term_ids), "br-ti", false);
            let ed_b = self.storage_init(&super::u32_to_bytes(edge_data), "br-ed", false);
            let eo_b = self.storage_init(&super::u32pairs_to_bytes(edge_off), "br-eo", false);

            let bridging_b = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("br-out"),
                size: (n as usize * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

            let params_bytes: Vec<u8> = [n, 0u32, 0u32, 0u32]
                .iter()
                .flat_map(|v| v.to_ne_bytes())
                .collect();
            let params_b = self.uniform_init(&params_bytes, "br-p");

            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("br-bg"),
                layout: &self.bridge_pipe.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: so_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: ti_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: ed_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: eo_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: bridging_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: params_b.as_entire_binding(),
                    },
                ],
            });

            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("br-enc"),
                });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("br-pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.bridge_pipe);
                pass.set_bind_group(0, &bg, &[]);
                pass.dispatch_workgroups(n.div_ceil(32), 1, 1);
            }
            self.queue.submit(std::iter::once(enc.finish()));
            self.readback_f32(&bridging_b, n as usize)
        }

        pub fn dispatch_score(
            &self,
            q_ids: &[u32],
            q_idfs: &[f32],
            cands: &[u32],
            sym_offsets: &[[u32; 2]],
            sym_tids: &[u32],
            sym_tfs: &[f32],
        ) -> Option<Vec<f32>> {
            let nc = cands.len() as u32;
            let nq = q_ids.len() as u32;

            let qi_b = self.storage_init(&super::u32_to_bytes(q_ids), "sc-qi", false);
            let qf_b = self.storage_init(&super::f32_to_bytes(q_idfs), "sc-qf", false);
            let so_b = self.storage_init(&super::u32pairs_to_bytes(sym_offsets), "sc-so", false);
            let st_b = self.storage_init(&super::u32_to_bytes(sym_tids), "sc-st", false);
            let sf_b = self.storage_init(&super::f32_to_bytes(sym_tfs), "sc-sf", false);
            let ca_b = self.storage_init(&super::u32_to_bytes(cands), "sc-ca", false);

            let scores_b = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sc-out"),
                size: (nc as usize * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

            let params_bytes: Vec<u8> = [nc, nq, 0u32, 0u32]
                .iter()
                .flat_map(|v| v.to_ne_bytes())
                .collect();
            let params_b = self.uniform_init(&params_bytes, "sc-p");

            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sc-bg"),
                layout: &self.score_pipe.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: qi_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: qf_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: so_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: st_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: sf_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: ca_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: scores_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: params_b.as_entire_binding(),
                    },
                ],
            });

            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("sc-enc"),
                });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("sc-pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.score_pipe);
                pass.set_bind_group(0, &bg, &[]);
                pass.dispatch_workgroups(nc.div_ceil(64), 1, 1);
            }
            self.queue.submit(std::iter::once(enc.finish()));
            self.readback_f32(&scores_b, nc as usize)
        }

        pub fn dispatch_term_match(
            &self,
            query: &super::PackedGpuQuery,
            candidates: &[usize],
            data: &super::GpuIndexData,
        ) -> Option<(Vec<f32>, Vec<u32>)> {
            let nc = candidates.len() as u32;
            let nq = query.query_idfs.len() as u32;
            if nc == 0 || nq == 0 || query.probe_ids.is_empty() || !data.is_compatible() {
                return None;
            }

            let q_idfs_b =
                self.storage_init(&super::f32_to_bytes(&query.query_idfs), "tm-q-idfs", false);
            let q_offsets_b = self.storage_init(
                &super::u32pairs_to_bytes(&query.query_offsets),
                "tm-q-offsets",
                false,
            );
            let q_probes_b = self.storage_init(
                &super::query_probes_to_bytes(&query.probe_ids, &query.probe_weights),
                "tm-q-probes",
                false,
            );
            // Keep the compact index vectors resident on the device after the
            // first search. Each query uploads only its sparse probes and the
            // candidate IDs, while the candidate list still bounds shader work.
            let resident = self.resident_index(data);
            let s_offsets_b = &resident.symbol_offsets;
            let s_terms_b = &resident.term_ids;
            let s_weights_b = &resident.term_weights;
            let candidate_ids: Vec<u32> = candidates.iter().map(|&index| index as u32).collect();
            let candidates_b =
                self.storage_init(&super::u32_to_bytes(&candidate_ids), "tm-candidates", false);
            let results_b = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tm-results"),
                size: (nc as u64) * 8,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let params_bytes: Vec<u8> = [nc, nq, 0u32, 0u32]
                .iter()
                .flat_map(|value| value.to_ne_bytes())
                .collect();
            let params_b = self.uniform_init(&params_bytes, "tm-params");

            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tm-bind-group"),
                layout: &self.term_match_pipe.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: q_idfs_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: q_offsets_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: q_probes_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: s_offsets_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: s_terms_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: s_weights_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: candidates_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: results_b.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: params_b.as_entire_binding(),
                    },
                ],
            });

            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("tm-encoder"),
                });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("tm-pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.term_match_pipe);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(nc.div_ceil(64), 1, 1);
            }
            self.queue.submit(std::iter::once(encoder.finish()));
            self.readback_matches(&results_b, candidates.len())
        }
    }
}

#[cfg(feature = "gpu")]
pub use wgpu_backend::GpuContext;

#[cfg(feature = "gpu")]
pub fn shared_context() -> Option<std::sync::Arc<GpuContext>> {
    use std::sync::{Arc, OnceLock};

    static CONTEXT: OnceLock<Option<Arc<GpuContext>>> = OnceLock::new();
    CONTEXT
        .get_or_init(|| {
            if env_flag("GRAPHIQ_DISABLE_GPU") {
                eprintln!("  GPU: disabled by GRAPHIQ_DISABLE_GPU; using CPU+rayon");
                return None;
            }
            match GpuContext::new() {
                Some(context) => {
                    eprintln!("  GPU: initialized {}", context.info());
                    Some(Arc::new(context))
                }
                None => {
                    eprintln!("  GPU: unavailable; using CPU+rayon");
                    None
                }
            }
        })
        .clone()
}

#[cfg(not(feature = "gpu"))]
pub fn shared_context() -> Option<()> {
    None
}

pub fn gpu_status() -> GpuStatus {
    #[cfg(feature = "gpu")]
    {
        if env_flag("GRAPHIQ_DISABLE_GPU") {
            return GpuStatus::Unavailable("disabled by GRAPHIQ_DISABLE_GPU".to_string());
        }
        match shared_context() {
            Some(context) => GpuStatus::Available(context.info().clone()),
            None => GpuStatus::Unavailable("no compatible adapter".to_string()),
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        GpuStatus::Disabled
    }
}

#[cfg(feature = "gpu")]
pub fn try_gpu_term_match(
    data: &GpuIndexData,
    query_terms: &[crate::cruncher::QueryTerm],
    candidates: &[usize],
) -> Option<GpuSearchResult> {
    if candidates.is_empty()
        || query_terms.is_empty()
        || candidates
            .iter()
            .any(|&candidate| candidate >= data.symbol_offsets.len())
    {
        return None;
    }

    let candidate_term_count: usize = candidates
        .iter()
        .map(|&candidate| data.symbol_offsets[candidate][1] as usize)
        .sum();
    if candidates.len() < gpu_min_search_candidates()
        || candidate_term_count.saturating_mul(query_terms.len().max(1))
            < gpu_min_search_work_items()
    {
        return None;
    }

    if !data.is_compatible() {
        return None;
    }

    let query = data.pack_query(query_terms);
    if query.probe_ids.is_empty()
        || !should_use_gpu_for_search(
            candidates.len(),
            query.probe_ids.len(),
            candidate_term_count,
        )
    {
        return None;
    }

    let context = shared_context()?;
    let started = std::time::Instant::now();
    let dispatch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        context.dispatch_term_match(&query, candidates, data)
    }))
    .ok()
    .flatten()?;
    let (scores, matched) = dispatch;
    if scores.len() != candidates.len() || matched.len() != candidates.len() {
        return None;
    }
    GPU_SEARCH_DISPATCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Some(GpuSearchResult {
        candidate_indices: candidates.to_vec(),
        scores,
        matched,
        elapsed_ms: started.elapsed().as_millis() as u64,
        device: context.info().clone(),
    })
}

#[cfg(not(feature = "gpu"))]
pub fn try_gpu_term_match(
    _data: &GpuIndexData,
    _query_terms: &[crate::cruncher::QueryTerm],
    _candidates: &[usize],
) -> Option<GpuSearchResult> {
    None
}

// ---------------------------------------------------------------------------
// Byte conversion helpers
// ---------------------------------------------------------------------------

pub fn f32_to_bytes(data: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 4);
    for v in data {
        out.extend_from_slice(&v.to_ne_bytes());
    }
    out
}

pub fn u32_to_bytes(data: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 4);
    for v in data {
        out.extend_from_slice(&v.to_ne_bytes());
    }
    out
}

pub fn u32pairs_to_bytes(pairs: &[[u32; 2]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pairs.len() * 8);
    for [a, b] in pairs {
        out.extend_from_slice(&a.to_ne_bytes());
        out.extend_from_slice(&b.to_ne_bytes());
    }
    out
}

pub fn query_probes_to_bytes(ids: &[u32], weights: &[f32]) -> Vec<u8> {
    debug_assert_eq!(ids.len(), weights.len());
    let mut out = Vec::with_capacity(ids.len() * 8);
    for (&id, &weight) in ids.iter().zip(weights) {
        out.extend_from_slice(&id.to_ne_bytes());
        out.extend_from_slice(&weight.to_ne_bytes());
    }
    out
}

pub fn bytes_to_f32(data: &[u8]) -> Vec<f32> {
    data.chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

// ---------------------------------------------------------------------------
// Flat cruncher data — intermediate between HashMap world and GPU arrays
// ---------------------------------------------------------------------------

pub struct FlatCruncherData {
    pub raw_counts: Vec<f32>,
    pub term_ids: Vec<u32>,
    pub symbol_offsets: Vec<[u32; 2]>,
    pub term_to_id: HashMap<String, u32>,
    pub id_to_term: HashMap<u32, String>,
    pub doc_freq_flat: Vec<f32>,
    pub outgoing_flat: Vec<u32>,
    pub outgoing_offsets: Vec<[u32; 2]>,
}

pub fn flatten_cruncher_data(
    raw_term_lists: &[Vec<(String, f64)>],
    n_symbols: usize,
    outgoing: &[Vec<crate::cruncher::Edge>],
) -> FlatCruncherData {
    let mut term_to_id: HashMap<String, u32> = HashMap::new();
    let mut next_id: u32 = 0;

    for terms in raw_term_lists {
        for (term, _) in terms {
            if !term_to_id.contains_key(term.as_str()) {
                term_to_id.insert(term.clone(), next_id);
                next_id += 1;
            }
        }
    }

    let mut id_to_term: HashMap<u32, String> = HashMap::with_capacity(term_to_id.len());
    for (term, &id) in &term_to_id {
        id_to_term.insert(id, term.clone());
    }

    let mut raw_counts = Vec::new();
    let mut term_ids = Vec::new();
    let mut symbol_offsets = Vec::with_capacity(n_symbols);

    for terms in raw_term_lists {
        let start = raw_counts.len() as u32;
        for (term, count) in terms {
            term_ids.push(term_to_id[term.as_str()]);
            raw_counts.push(*count as f32);
        }
        let len = (raw_counts.len() as u32) - start;
        symbol_offsets.push([start, len]);
    }

    let n_unique = term_to_id.len();
    let mut doc_freq_flat = vec![0.0f32; n_unique];
    for terms in raw_term_lists {
        let mut seen: HashMap<u32, bool> = HashMap::new();
        for (term, _) in terms {
            if let Some(&id) = term_to_id.get(term.as_str()) {
                if seen.insert(id, true).is_none() {
                    doc_freq_flat[id as usize] += 1.0;
                }
            }
        }
    }

    let mut outgoing_flat = Vec::new();
    let mut outgoing_offsets = Vec::with_capacity(n_symbols);
    for adj in outgoing {
        let start = outgoing_flat.len() as u32;
        for edge in adj.iter().take(20) {
            outgoing_flat.push(edge.target as u32);
        }
        let len = (outgoing_flat.len() as u32) - start;
        outgoing_offsets.push([start, len]);
    }

    FlatCruncherData {
        raw_counts,
        term_ids,
        symbol_offsets,
        term_to_id,
        id_to_term,
        doc_freq_flat,
        outgoing_flat,
        outgoing_offsets,
    }
}

// ---------------------------------------------------------------------------
// CPU fallback — rayon multithreaded
// ---------------------------------------------------------------------------

pub fn cpu_normalize_tf(counts: &mut [f32], offsets: &[[u32; 2]]) {
    for &[start, len] in offsets {
        let s = start as usize;
        let l = len as usize;
        if l == 0 {
            continue;
        }
        let total: f32 = counts[s..s + l].iter().sum();
        if total > 0.0 {
            for c in &mut counts[s..s + l] {
                *c /= total;
            }
        }
    }
}

pub fn cpu_compute_idf(doc_freq: &[f32], n_symbols: f64) -> Vec<f32> {
    doc_freq
        .par_iter()
        .map(|&df| (1.0 + n_symbols / (df as f64 + 1.0)).ln() as f32)
        .collect()
}

pub fn cpu_compute_bridging(
    sym_offsets: &[[u32; 2]],
    term_ids: &[u32],
    outgoing_flat: &[u32],
    outgoing_offsets: &[[u32; 2]],
) -> Vec<f64> {
    (0..sym_offsets.len())
        .into_par_iter()
        .map(|sym| {
            let [my_start, my_len] = sym_offsets[sym];
            let ms = my_start as usize;
            let ml = my_len as usize;
            if ml == 0 {
                return 0.0;
            }
            let own: std::collections::HashSet<u32> =
                term_ids[ms..ms + ml.min(64)].iter().copied().collect();

            let [es, el] = outgoing_offsets[sym];
            let es = es as usize;
            let el = el as usize;
            if el == 0 {
                return 0.0;
            }

            let mut novel = 0.0f64;
            let mut total = 0.0f64;

            for e in 0..el {
                let nb = outgoing_flat[es + e] as usize;
                if nb >= sym_offsets.len() {
                    continue;
                }
                let [ns, nl] = sym_offsets[nb];
                let ns = ns as usize;
                let nl = nl as usize;
                for t in 0..nl.min(30) {
                    let tid = term_ids[ns + t];
                    total += 1.0;
                    if !own.contains(&tid) {
                        novel += 1.0;
                    }
                }
            }

            let novelty = if total > 0.0 { novel / total } else { 0.0 };
            let boost = (1.0 + el as f64).ln_1p() * 0.3;
            novelty * (1.0 + boost)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Unified dispatch — GPU when available, CPU otherwise
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct GpuStats {
    pub tf_on_gpu: bool,
    pub idf_on_gpu: bool,
    pub bridge_on_gpu: bool,
    pub elapsed_ms: u64,
}

impl std::fmt::Display for GpuStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let gpu_count = [self.tf_on_gpu, self.idf_on_gpu, self.bridge_on_gpu]
            .iter()
            .filter(|&&b| b)
            .count();
        write!(
            f,
            "gpu acceleration: {}/3 phases on GPU ({:.0}ms)",
            gpu_count, self.elapsed_ms as f64,
        )
    }
}

pub struct ComputeResults {
    pub normalized_tf: Vec<f32>,
    pub idf_values: Vec<f32>,
    pub bridging: Vec<f64>,
    pub stats: GpuStats,
}

pub fn accelerated_compute(
    flat: &FlatCruncherData,
    n_symbols: usize,
    mut raw_counts: Vec<f32>,
) -> ComputeResults {
    use std::time::Instant;
    let t0 = Instant::now();
    let n_sym32 = n_symbols as u32;

    cpu_normalize_tf(&mut raw_counts, &flat.symbol_offsets);

    let idf_values: Vec<f32> = cpu_compute_idf(&flat.doc_freq_flat, n_sym32 as f64);

    let bridging: Vec<f64> = cpu_compute_bridging(
        &flat.symbol_offsets,
        &flat.term_ids,
        &flat.outgoing_flat,
        &flat.outgoing_offsets,
    );

    let elapsed_ms = t0.elapsed().as_millis() as u64;

    ComputeResults {
        normalized_tf: raw_counts,
        idf_values,
        bridging,
        stats: GpuStats {
            tf_on_gpu: false,
            idf_on_gpu: false,
            bridge_on_gpu: false,
            elapsed_ms,
        },
    }
}

#[cfg(feature = "gpu")]
pub fn accelerated_compute_gpu(
    flat: &FlatCruncherData,
    n_symbols: usize,
    raw_counts: Vec<f32>,
    gpu: &GpuContext,
) -> ComputeResults {
    use std::time::Instant;
    let t0 = Instant::now();
    let n_sym32 = n_symbols as u32;

    let (normalized, tf_on_gpu) = {
        let dispatch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            gpu.dispatch_tf(&raw_counts, &flat.symbol_offsets, n_sym32)
        }))
        .ok()
        .flatten();
        if let Some(result) = dispatch.filter(|result| result.len() == raw_counts.len()) {
            (result, true)
        } else {
            let mut c = raw_counts;
            cpu_normalize_tf(&mut c, &flat.symbol_offsets);
            (c, false)
        }
    };

    let (idf_values, idf_on_gpu) = {
        let dispatch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            gpu.dispatch_idf(&flat.doc_freq_flat, n_sym32)
        }))
        .ok()
        .flatten();
        if let Some(result) = dispatch.filter(|result| result.len() == flat.doc_freq_flat.len()) {
            (result, true)
        } else {
            (cpu_compute_idf(&flat.doc_freq_flat, n_sym32 as f64), false)
        }
    };

    let (bridging, bridge_on_gpu) = {
        if flat.outgoing_flat.is_empty() || flat.term_ids.is_empty() {
            (
                cpu_compute_bridging(
                    &flat.symbol_offsets,
                    &flat.term_ids,
                    &flat.outgoing_flat,
                    &flat.outgoing_offsets,
                ),
                false,
            )
        } else if let Some(result) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            gpu.dispatch_bridge(
                &flat.symbol_offsets,
                &flat.term_ids,
                &flat.outgoing_flat,
                &flat.outgoing_offsets,
            )
        }))
        .ok()
        .flatten()
        .filter(|result| result.len() == n_symbols)
        {
            let f64s: Vec<f64> = result.iter().map(|v| *v as f64).collect();
            (f64s, true)
        } else {
            (
                cpu_compute_bridging(
                    &flat.symbol_offsets,
                    &flat.term_ids,
                    &flat.outgoing_flat,
                    &flat.outgoing_offsets,
                ),
                false,
            )
        }
    };

    let elapsed_ms = t0.elapsed().as_millis() as u64;

    ComputeResults {
        normalized_tf: normalized,
        idf_values,
        bridging,
        stats: GpuStats {
            tf_on_gpu,
            idf_on_gpu,
            bridge_on_gpu,
            elapsed_ms,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    #[cfg(feature = "gpu")]
    use crate::cruncher::term_match_score;
    use crate::cruncher::{QueryTerm, TermSet};

    fn sample_term_sets() -> Vec<TermSet> {
        let mut first = HashMap::new();
        first.insert("request".to_string(), 0.5);
        first.insert("handler".to_string(), 0.25);

        let mut second = HashMap::new();
        second.insert("request_handler".to_string(), 1.0);

        vec![
            TermSet {
                terms: first,
                name_terms: ["request", "handler"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                sig_terms: HashSet::new(),
            },
            TermSet {
                terms: second,
                name_terms: HashSet::new(),
                sig_terms: HashSet::new(),
            },
        ]
    }

    #[test]
    fn packed_index_is_deterministic_and_well_formed() {
        let term_sets = sample_term_sets();
        let idf = [("request", 1.5), ("handler", 2.0), ("request_handler", 1.2)]
            .into_iter()
            .map(|(term, value)| (term.to_string(), value))
            .collect();
        let data = GpuIndexData::from_term_sets(&term_sets, &idf);

        assert!(data.is_compatible());
        assert_eq!(data.terms, vec!["handler", "request", "request_handler"]);
        assert_eq!(data.symbol_offsets, vec![[0, 2], [2, 1]]);
        assert_eq!(data.term_ids.len(), data.term_weights.len());
        assert_eq!(data.term_idf.len(), data.terms.len());
    }

    #[test]
    fn packed_query_preserves_variant_match_ratios() {
        let term_sets = sample_term_sets();
        let idf = [("request", 1.5), ("handler", 2.0), ("request_handler", 1.2)]
            .into_iter()
            .map(|(term, value)| (term.to_string(), value))
            .collect();
        let data = GpuIndexData::from_term_sets(&term_sets, &idf);
        let query = [QueryTerm {
            text: "request".into(),
            variants: vec!["request".into()],
            idf: 1.5,
        }];

        let packed = data.pack_query(&query);
        assert_eq!(packed.query_offsets, vec![[0, 2]]);
        assert_eq!(packed.probe_ids.len(), 2);
        assert!(packed.probe_weights.iter().any(|weight| (*weight
            - ("request".len() as f32 / "request_handler".len() as f32))
            .abs()
            < 1e-6));
    }

    #[test]
    fn packed_index_round_trips_and_rejects_unknown_versions() {
        let term_sets = sample_term_sets();
        let idf = [("request", 1.5), ("handler", 2.0), ("request_handler", 1.2)]
            .into_iter()
            .map(|(term, value)| (term.to_string(), value))
            .collect();
        let data = GpuIndexData::from_term_sets(&term_sets, &idf);
        let encoded = bincode::serialize(&data).expect("packed index should serialize");
        let decoded: GpuIndexData =
            bincode::deserialize(&encoded).expect("packed index should deserialize");
        assert!(decoded.is_compatible());

        let mut incompatible = decoded;
        incompatible.version += 1;
        assert!(!incompatible.is_compatible());
    }

    #[cfg(feature = "gpu")]
    #[test]
    fn gpu_term_matching_matches_cpu_when_adapter_is_available() {
        let term_sets = sample_term_sets();
        let idf = [("request", 1.5), ("handler", 2.0), ("request_handler", 1.2)]
            .into_iter()
            .map(|(term, value)| (term.to_string(), value))
            .collect();
        let data = GpuIndexData::from_term_sets(&term_sets, &idf);
        let query = vec![
            QueryTerm {
                text: "request".into(),
                variants: vec!["request".into()],
                idf: 1.5,
            },
            QueryTerm {
                text: "handler".into(),
                variants: vec!["handler".into()],
                idf: 2.0,
            },
        ];
        let context = match GpuContext::new() {
            Some(context) => context,
            None => return,
        };
        let packed = data.pack_query(&query);
        let (gpu_scores, gpu_matched) = context
            .dispatch_term_match(&packed, &[0, 1], &data)
            .expect("Metal/Vulkan dispatch should return readback data");

        for (index, term_set) in term_sets.iter().enumerate() {
            let (cpu_score, cpu_matched) = term_match_score(&query, term_set);
            assert!((gpu_scores[index] as f64 - cpu_score).abs() < 1e-6);
            assert_eq!(gpu_matched[index] as usize, cpu_matched);
        }
    }

    #[cfg(feature = "gpu")]
    #[test]
    fn gpu_index_phases_match_cpu_when_adapter_is_available() {
        let terms = vec![
            vec![("request".to_string(), 2.0), ("handler".to_string(), 1.0)],
            vec![("request".to_string(), 1.0), ("cache".to_string(), 3.0)],
        ];
        let outgoing = vec![
            vec![crate::cruncher::Edge {
                target: 1,
                weight: 1.0,
                kind_weight: 1.0,
            }],
            Vec::new(),
        ];
        let flat = flatten_cruncher_data(&terms, 2, &outgoing);
        let context = match GpuContext::new() {
            Some(context) => context,
            None => return,
        };
        let cpu = accelerated_compute(&flat, 2, flat.raw_counts.clone());
        let gpu = accelerated_compute_gpu(&flat, 2, flat.raw_counts.clone(), &context);

        assert_eq!(gpu.normalized_tf.len(), cpu.normalized_tf.len());
        for (gpu_value, cpu_value) in gpu.normalized_tf.iter().zip(cpu.normalized_tf.iter()) {
            assert!((*gpu_value - *cpu_value).abs() < 1e-6);
        }
        for (gpu_value, cpu_value) in gpu.idf_values.iter().zip(cpu.idf_values.iter()) {
            assert!((*gpu_value - *cpu_value).abs() < 1e-6);
        }
        for (gpu_value, cpu_value) in gpu.bridging.iter().zip(cpu.bridging.iter()) {
            assert!(
                (*gpu_value - *cpu_value).abs() < 1e-5,
                "gpu={gpu_value} cpu={cpu_value}"
            );
        }
    }
}
