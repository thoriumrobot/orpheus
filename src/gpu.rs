//! gpu — a data-parallel compute library for Latte.
//!
//! This exposes a GPU *programming model* (device, buffers, and data-parallel kernels:
//! map / zipWith / reduce / saxpy / dot / matmul, plus a per-pixel field shader) with a
//! pluggable backend. The TARGET device is an NVIDIA GeForce RTX 4070 Ti SUPER; a real
//! CUDA backend would dispatch these kernels to it. In THIS environment there is no CUDA
//! driver and the project carries zero external crates, so the active backend is a genuine
//! multi-core CPU backend that runs each kernel in parallel across the host's cores with
//! `std::thread::scope`. The kernel set, buffer model, and call sites are identical to what
//! a CUDA backend would use, so swapping in a device backend is a drop-in change.
//!
//! It integrates with the ML library (matmul is the core neural-net op — see `nn.lat`/`ml.lat`)
//! and the graphics library (a parallel field shader produces a `gfx` scene rendered to SVG).

use std::thread;

/// Which compute backend is actually executing kernels.
#[derive(Clone, Debug, PartialEq)]
pub enum Backend {
    /// A CUDA-capable NVIDIA GPU was detected (its name). Real dispatch needs a CUDA build,
    /// which this zero-dependency binary does not link; selection logic still reports it.
    Cuda(String),
    /// The portable multi-core CPU backend (`std::thread`), with this many hardware lanes.
    Cpu(usize),
}

/// Probe the host for an NVIDIA GPU. Pure filesystem/PATH checks — no external crates, never
/// panics. Returns `Backend::Cuda(name)` if a device is present, else `Backend::Cpu(lanes)`.
/// This is the "detect by default" entry point: callers accelerate on the GPU when present and
/// fall back to the CPU backend otherwise, never relying on a GPU that is not there.
pub fn detect_backend() -> Backend {
    let lanes = thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    // 1) NVIDIA kernel driver exposes /proc/driver/nvidia/gpus/*/information and /dev/nvidiaN
    if let Ok(rd) = std::fs::read_dir("/proc/driver/nvidia/gpus") {
        for e in rd.flatten() {
            let info = e.path().join("information");
            if let Ok(txt) = std::fs::read_to_string(&info) {
                if let Some(line) = txt.lines().find(|l| l.starts_with("Model:")) {
                    return Backend::Cuda(line.trim_start_matches("Model:").trim().to_string());
                }
            }
            return Backend::Cuda("NVIDIA GPU".to_string());
        }
    }
    if std::path::Path::new("/dev/nvidia0").exists() {
        return Backend::Cuda("NVIDIA GPU".to_string());
    }
    // 2) nvidia-smi on PATH is a strong signal a usable driver is installed
    if let Ok(paths) = std::env::var("PATH") {
        for dir in paths.split(':') {
            if std::path::Path::new(dir).join("nvidia-smi").exists() {
                return Backend::Cuda("NVIDIA GPU (nvidia-smi present)".to_string());
            }
        }
    }
    Backend::Cpu(lanes)
}

/// The compute device. `target` describes the intended GPU; `backend`/`lanes` describe what is
/// actually executing the kernels in this environment.
pub struct Device {
    pub target: &'static str,
    pub vram_gb: u32,
    pub cuda_cores: u32,
    pub sm_count: u32,
    pub backend: String,
    pub lanes: usize,
    pub gpu_present: bool,
}

impl Device {
    /// The configured target card, with the *detected* backend reported honestly.
    pub fn target() -> Device {
        let detected = detect_backend();
        let lanes = thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let (backend, gpu_present) = match &detected {
            Backend::Cuda(name) => (
                format!("CUDA device detected ({}) — would dispatch kernels to GPU", name),
                true,
            ),
            Backend::Cpu(l) => (
                format!("no GPU detected — multi-core CPU backend ({} threads)", l),
                false,
            ),
        };
        Device {
            target: "NVIDIA GeForce RTX 4070 Ti SUPER",
            vram_gb: 16, // the 4070 Ti SUPER ships with 16 GB GDDR6X (not 32)
            cuda_cores: 8448,
            sm_count: 66,
            backend,
            lanes,
            gpu_present,
        }
    }
}

/// Split `n` items into roughly equal contiguous chunks, one per lane.
fn chunks(n: usize, lanes: usize) -> Vec<(usize, usize)> {
    let lanes = lanes.max(1).min(n.max(1));
    let base = n / lanes;
    let rem = n % lanes;
    let mut out = Vec::with_capacity(lanes);
    let mut start = 0;
    for i in 0..lanes {
        let len = base + if i < rem { 1 } else { 0 };
        if len == 0 {
            continue;
        }
        out.push((start, start + len));
        start += len;
    }
    out
}

/// Parallel elementwise map: `out[i] = f(a[i])`.
pub fn pmap(a: &[f64], lanes: usize, f: impl Fn(f64) -> f64 + Sync) -> Vec<f64> {
    let mut out = vec![0.0; a.len()];
    let ranges = chunks(a.len(), lanes);
    thread::scope(|s| {
        // split by index ranges; each thread writes a disjoint &mut sub-slice
        let mut rest = out.as_mut_slice();
        let fref = &f;
        for &(lo, hi) in &ranges {
            let (head, tail) = rest.split_at_mut(hi - lo);
            rest = tail;
            let src = &a[lo..hi];
            s.spawn(move || {
                for (o, &x) in head.iter_mut().zip(src.iter()) {
                    *o = fref(x);
                }
            });
        }
    });
    out
}

/// Parallel elementwise binary op: `out[i] = f(a[i], b[i])`.
pub fn pzip(a: &[f64], b: &[f64], lanes: usize, f: impl Fn(f64, f64) -> f64 + Sync) -> Vec<f64> {
    let n = a.len().min(b.len());
    let mut out = vec![0.0; n];
    let ranges = chunks(n, lanes);
    thread::scope(|s| {
        let mut rest = out.as_mut_slice();
        let fref = &f;
        for &(lo, hi) in &ranges {
            let (head, tail) = rest.split_at_mut(hi - lo);
            rest = tail;
            let sa = &a[lo..hi];
            let sb = &b[lo..hi];
            s.spawn(move || {
                for i in 0..head.len() {
                    head[i] = fref(sa[i], sb[i]);
                }
            });
        }
    });
    out
}

/// Parallel reduction (sum) with per-lane partials.
pub fn preduce_sum(a: &[f64], lanes: usize) -> f64 {
    let ranges = chunks(a.len(), lanes);
    let partials: Vec<f64> = thread::scope(|s| {
        let mut handles = Vec::new();
        for &(lo, hi) in &ranges {
            let slice = &a[lo..hi];
            handles.push(s.spawn(move || slice.iter().sum::<f64>()));
        }
        handles.into_iter().map(|h| h.join().unwrap_or(0.0)).collect()
    });
    partials.iter().sum()
}

/// saxpy: `y = alpha*x + y` (in parallel).
pub fn saxpy(alpha: f64, x: &[f64], y: &[f64], lanes: usize) -> Vec<f64> {
    pzip(x, y, lanes, move |xi, yi| alpha * xi + yi)
}

/// dot product via parallel multiply + reduce.
pub fn dot(a: &[f64], b: &[f64], lanes: usize) -> f64 {
    let prod = pzip(a, b, lanes, |x, y| x * y);
    preduce_sum(&prod, lanes)
}

/// Dense matrix multiply `C = A(m×k) · B(k×n)`, parallelized over rows of A.
/// Row-major flat slices. This is the core neural-network kernel (see `nn.lat`).
pub fn matmul(a: &[f64], b: &[f64], m: usize, k: usize, n: usize, lanes: usize) -> Vec<f64> {
    let mut c = vec![0.0; m * n];
    let ranges = chunks(m, lanes);
    thread::scope(|s| {
        let mut rest = c.as_mut_slice();
        for &(lo, hi) in &ranges {
            let (head, tail) = rest.split_at_mut((hi - lo) * n);
            rest = tail;
            s.spawn(move || {
                for (ri, row) in (lo..hi).enumerate() {
                    for col in 0..n {
                        let mut acc = 0.0;
                        for p in 0..k {
                            acc += a[row * k + p] * b[p * n + col];
                        }
                        head[ri * n + col] = acc;
                    }
                }
            });
        }
    });
    c
}

/// Serial matmul reference (for the parallel-vs-serial benchmark and tests).
pub fn matmul_serial(a: &[f64], b: &[f64], m: usize, k: usize, n: usize) -> Vec<f64> {
    let mut c = vec![0.0; m * n];
    for row in 0..m {
        for col in 0..n {
            let mut acc = 0.0;
            for p in 0..k {
                acc += a[row * k + p] * b[p * n + col];
            }
            c[row * n + col] = acc;
        }
    }
    c
}

/// A per-pixel "shader": the escape-time Mandelbrot set, one independent thread of work per
/// pixel — the canonical embarrassingly-parallel GPU workload. Returns packed-RGB per pixel.
pub fn mandelbrot(w: usize, h: usize, max_iter: u32, lanes: usize) -> Vec<u32> {
    let mut out = vec![0u32; w * h];
    let ranges = chunks(h, lanes);
    thread::scope(|s| {
        let mut rest = out.as_mut_slice();
        for &(lo, hi) in &ranges {
            let (head, tail) = rest.split_at_mut((hi - lo) * w);
            rest = tail;
            s.spawn(move || {
                for (ri, py) in (lo..hi).enumerate() {
                    let y0 = (py as f64 / h as f64) * 2.4 - 1.2;
                    for px in 0..w {
                        let x0 = (px as f64 / w as f64) * 3.2 - 2.1;
                        let (mut x, mut y, mut it) = (0.0f64, 0.0f64, 0u32);
                        while x * x + y * y <= 4.0 && it < max_iter {
                            let xt = x * x - y * y + x0;
                            y = 2.0 * x * y + y0;
                            x = xt;
                            it += 1;
                        }
                        head[ri * w + px] = color_ramp(it, max_iter);
                    }
                }
            });
        }
    });
    out
}

/// Map an escape count to a packed RGB colour.
fn color_ramp(it: u32, max_iter: u32) -> u32 {
    if it >= max_iter {
        return 0x101020;
    }
    let t = it as f64 / max_iter as f64;
    let r = (9.0 * (1.0 - t) * t * t * t * 255.0) as u32;
    let g = (15.0 * (1.0 - t) * (1.0 - t) * t * t * 255.0) as u32;
    let b = (8.5 * (1.0 - t) * (1.0 - t) * (1.0 - t) * t * 255.0) as u32;
    (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)
}

/// Turn a colour field into a `gfx` SVG raster (a grid of cells), integrating GPU + GFX.
pub fn field_to_svg(field: &[u32], w: usize, h: usize, cell: usize) -> String {
    let mut body = String::new();
    for py in 0..h {
        for px in 0..w {
            let c = field[py * w + px];
            body += &format!(
                "<rect x='{}' y='{}' width='{}' height='{}' fill='#{:06x}'/>",
                px * cell, py * cell, cell, cell, c & 0xffffff
            );
        }
    }
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}'>{}</svg>",
        w * cell, h * cell, body
    )
}

/// Auto-dispatched matmul: use the GPU when detected, else the CPU backend. In this
/// zero-dependency build there is no CUDA link, so a detected GPU still computes via the
/// parallel CPU path (correct results), while a real CUDA build would offload here.
pub fn matmul_auto(a: &[f64], b: &[f64], m: usize, k: usize, n: usize) -> Vec<f64> {
    let lanes = match detect_backend() {
        Backend::Cuda(_) => thread::available_parallelism().map(|x| x.get()).unwrap_or(1),
        Backend::Cpu(l) => l,
    };
    matmul(a, b, m, k, n, lanes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_detection_is_sound() {
        // Never panics and returns a usable backend; CPU backend reports >=1 lane.
        match detect_backend() {
            Backend::Cpu(l) => assert!(l >= 1),
            Backend::Cuda(name) => assert!(!name.is_empty()),
        }
        // matmul_auto must agree with the serial reference regardless of backend.
        let (m, k, n) = (8, 6, 7);
        let a: Vec<f64> = (0..m * k).map(|i| (i % 5) as f64).collect();
        let b: Vec<f64> = (0..k * n).map(|i| (i % 3) as f64).collect();
        assert_eq!(matmul_auto(&a, &b, m, k, n), matmul_serial(&a, &b, m, k, n));
    }

    #[test]
    fn matmul_parallel_matches_serial() {
        let m = 17;
        let k = 13;
        let n = 19;
        let a: Vec<f64> = (0..m * k).map(|i| (i % 7) as f64 - 3.0).collect();
        let b: Vec<f64> = (0..k * n).map(|i| (i % 5) as f64 - 2.0).collect();
        let par = matmul(&a, &b, m, k, n, 4);
        let ser = matmul_serial(&a, &b, m, k, n);
        assert_eq!(par, ser, "parallel matmul must equal serial reference");
    }

    #[test]
    fn parallel_primitives() {
        let a: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        assert_eq!(preduce_sum(&a, 8), 5050.0);
        let b = vec![2.0; 100];
        let y = saxpy(3.0, &a, &b, 8); // 3*i + 2
        assert_eq!(y[0], 5.0);
        assert_eq!(y[99], 302.0);
        assert_eq!(dot(&a, &b, 8), 5050.0 * 2.0);
        let mapped = pmap(&a, 8, |x| x * x);
        assert_eq!(mapped[9], 100.0);
    }

    #[test]
    fn mandelbrot_shape_and_interior() {
        let w = 40;
        let h = 30;
        let field = mandelbrot(w, h, 50, 4);
        assert_eq!(field.len(), w * h);
        // the origin (~centre) is inside the set -> interior colour
        let cx = (((-0.0 + 2.1) / 3.2) * w as f64) as usize;
        let cy = (((0.0 + 1.2) / 2.4) * h as f64) as usize;
        assert_eq!(field[cy * w + cx] & 0xffffff, 0x101020);
        let svg = field_to_svg(&field, w, h, 2);
        assert!(svg.contains("<rect"));
    }
}
