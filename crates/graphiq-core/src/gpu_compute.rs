//! Cross-platform GPU compute acceleration for graphiq background indexing.
//!
//! Delegates data-parallel cruncher operations (TF normalization, IDF
//! computation, bridging potential) from CPU RAM to GPU VRAM via wgpu
//! compute shaders — Metal on macOS, Vulkan on Linux, DX12 on Windows.
//!
//! Falls back to rayon CPU multithreading when no GPU is available.
//! The GPU path reduces main-memory pressure by keeping intermediate
//! arrays in VRAM during the heaviest cruncher phases.

use std::collections::HashMap;

use rayon::prelude::*;

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
    let boost = log2(1.0 + f32(el)) * 0.3;
    bridging[sym] = novelty * (1.0 + boost);
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
    use std::sync::Arc;

    pub struct GpuContext {
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        tf_pipe: wgpu::ComputePipeline,
        idf_pipe: wgpu::ComputePipeline,
        bridge_pipe: wgpu::ComputePipeline,
        score_pipe: wgpu::ComputePipeline,
    }

    impl GpuContext {
        pub fn new() -> Option<Self> {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Self::init_inner()));
            result.ok().flatten()
        }

        fn init_inner() -> Option<Self> {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..Default::default()
            });

            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                }))?;

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
            let score_pipe = Self::make_pipeline(&device, super::SCORE_SHADER, "score");

            Some(Self {
                device,
                queue,
                tf_pipe,
                idf_pipe,
                bridge_pipe,
                score_pipe,
            })
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
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
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

        fn readback_f32(&self, buf: &wgpu::Buffer, count: usize) -> Option<Vec<f32>> {
            let byte_len = (count * 4) as u64;
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
            let _ = rx.recv().ok()?;

            let view = slice.get_mapped_range();
            let out = super::bytes_to_f32(&view);
            drop(view);
            staging.unmap();
            Some(out)
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
    }
}

#[cfg(feature = "gpu")]
pub use wgpu_backend::GpuContext;

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
        if let Some(result) = gpu.dispatch_tf(&raw_counts, &flat.symbol_offsets, n_sym32) {
            (result, true)
        } else {
            let mut c = raw_counts;
            cpu_normalize_tf(&mut c, &flat.symbol_offsets);
            (c, false)
        }
    };

    let (idf_values, idf_on_gpu) = {
        if let Some(result) = gpu.dispatch_idf(&flat.doc_freq_flat, n_sym32) {
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
        } else if let Some(result) = gpu.dispatch_bridge(
            &flat.symbol_offsets,
            &flat.term_ids,
            &flat.outgoing_flat,
            &flat.outgoing_offsets,
        ) {
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
