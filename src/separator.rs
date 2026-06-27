use std::path::Path;

use ndarray::{Array2, Array4};
use num_complex::Complex32;
use ort::session::Session;
use ort::value::Value;
use realfft::RealFftPlanner;

const N_FFT: usize = 6144;
const HOP_LENGTH: usize = 1024;
fn hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|i| {
            let x = std::f32::consts::PI * i as f32 / size as f32;
            x.sin().powi(2)
        })
        .collect()
}

fn stft(signal: &[f32], n_fft: usize, hop_length: usize, window: &[f32]) -> Array2<Complex32> {
    let pad = n_fft / 2;
    let padded_len = signal.len() + 2 * pad;
    let mut padded = vec![0.0f32; padded_len];
    padded[pad..pad + signal.len()].copy_from_slice(signal);

    let n_frames = (padded_len - n_fft) / hop_length + 1;
    let freq_bins = n_fft / 2 + 1;

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n_fft);

    let mut result = Array2::<Complex32>::zeros((freq_bins, n_frames));

    let mut frame_buf = vec![0.0f32; n_fft];
    let mut spectrum = vec![Complex32::new(0.0, 0.0); freq_bins];

    for t in 0..n_frames {
        let start = t * hop_length;
        for i in 0..n_fft {
            frame_buf[i] = padded[start + i] * window[i];
        }
        fft.process(&mut frame_buf, &mut spectrum).unwrap();
        for f in 0..freq_bins {
            result[[f, t]] = spectrum[f];
        }
    }

    result
}

fn istft(
    spectrogram: &Array2<Complex32>,
    n_fft: usize,
    hop_length: usize,
    window: &[f32],
    output_length: usize,
) -> Vec<f32> {
    let freq_bins = spectrogram.shape()[0];
    let n_frames = spectrogram.shape()[1];

    let pad = n_fft / 2;
    let padded_len = output_length + 2 * pad;

    let mut planner = RealFftPlanner::<f32>::new();
    let ifft = planner.plan_fft_inverse(n_fft);

    let mut output = vec![0.0f32; padded_len];
    let mut window_sum = vec![0.0f32; padded_len];

    let mut spectrum = vec![Complex32::new(0.0, 0.0); freq_bins];
    let mut frame_buf = vec![0.0f32; n_fft];

    for t in 0..n_frames {
        for f in 0..freq_bins {
            spectrum[f] = spectrogram[[f, t]];
        }
        // realfft requires DC and Nyquist bins to have zero imaginary part
        spectrum[0].im = 0.0;
        spectrum[freq_bins - 1].im = 0.0;
        ifft.process(&mut spectrum, &mut frame_buf).unwrap();

        let norm = 1.0 / n_fft as f32;
        let start = t * hop_length;
        for i in 0..n_fft {
            if start + i < padded_len {
                output[start + i] += frame_buf[i] * norm * window[i];
                window_sum[start + i] += window[i] * window[i];
            }
        }
    }

    for i in 0..padded_len {
        if window_sum[i] > 1e-8 {
            output[i] /= window_sum[i];
        }
    }

    output[pad..pad + output_length].to_vec()
}

pub struct MdxSeparator {
    session: Session,
    dim_t: usize,
    dim_f: usize,
    n_channels: usize,
    window: Vec<f32>,
}

impl MdxSeparator {
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let session = Session::builder()
            .map_err(|e| format!("Failed to create ONNX session builder: {}", e))?
            .with_intra_threads(num_cpus())
            .map_err(|e| format!("Failed to set threads: {}", e))?
            .commit_from_file(model_path)
            .map_err(|e| format!("Failed to load ONNX model: {}", e))?;

        // Read model input shape: [batch, n_channels, dim_f, dim_t]
        let (n_channels, dim_f, dim_t) = match session.inputs()[0].dtype() {
            ort::value::ValueType::Tensor { shape, .. } => {
                if shape.len() == 4 {
                    let nc = if shape[1] > 0 { shape[1] as usize } else { 4 };
                    let df = if shape[2] > 0 { shape[2] as usize } else { 3072 };
                    let dt = if shape[3] > 0 { shape[3] as usize } else { 256 };
                    (nc, df, dt)
                } else {
                    (4, 3072, 256)
                }
            }
            _ => (4, 3072, 256),
        };

        let window = hann_window(N_FFT);

        Ok(Self {
            session,
            dim_t,
            dim_f,
            n_channels,
            window,
        })
    }

    pub fn separate(
        &mut self,
        audio: &[Vec<f32>],
        progress: Option<&dyn Fn(f32)>,
    ) -> Result<(Vec<Vec<f32>>, Vec<Vec<f32>>), String> {
        let sample_len = audio[0].len();

        // Ensure stereo
        let stereo: Vec<Vec<f32>> = if audio.len() == 1 {
            vec![audio[0].clone(), audio[0].clone()]
        } else {
            vec![audio[0].clone(), audio[1].clone()]
        };

        // STFT for each channel
        let specs: Vec<Array2<Complex32>> = stereo
            .iter()
            .map(|ch| stft(ch, N_FFT, HOP_LENGTH, &self.window))
            .collect();

        let freq_bins = specs[0].shape()[0]; // n_fft/2 + 1 = 3073
        let n_frames = specs[0].shape()[1];
        let dim_f = self.dim_f.min(freq_bins); // 3072

        // Process in chunks with overlap
        let overlap = self.dim_t / 2;
        let step = self.dim_t - overlap;
        let n_chunks = if n_frames <= self.dim_t {
            1
        } else {
            (n_frames - overlap + step - 1) / step
        };

        // Accumulator for output (same shape as input: 4 channels of [dim_f, n_frames])
        let mut output_acc = vec![Array2::<f32>::zeros((dim_f, n_frames)); self.n_channels];
        let mut weight_sum = vec![0.0f32; n_frames];

        // Triangular blend window for overlap-add
        let blend_window: Vec<f32> = (0..self.dim_t)
            .map(|i| {
                let mid = self.dim_t as f32 / 2.0;
                1.0 - (i as f32 - mid).abs() / mid
            })
            .collect();

        for chunk_idx in 0..n_chunks {
            let start = chunk_idx * step;
            let end = (start + self.dim_t).min(n_frames);
            let chunk_len = end - start;

            // Build input: [1, n_channels, dim_f, dim_t]
            // n_channels=4: [ch0_real, ch0_imag, ch1_real, ch1_imag]
            let mut input_data = Array4::<f32>::zeros((1, self.n_channels, dim_f, self.dim_t));
            for f in 0..dim_f {
                for t in 0..chunk_len {
                    let c0 = specs[0][[f, start + t]];
                    let c1 = specs[1][[f, start + t]];
                    input_data[[0, 0, f, t]] = c0.re;
                    input_data[[0, 1, f, t]] = c0.im;
                    input_data[[0, 2, f, t]] = c1.re;
                    input_data[[0, 3, f, t]] = c1.im;
                }
            }

            let input_value = Value::from_array(input_data)
                .map_err(|e| format!("Failed to create input tensor: {}", e))?;

            let outputs = self
                .session
                .run(ort::inputs![input_value])
                .map_err(|e| format!("Inference failed: {}", e))?;

            let output_tensor = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("Failed to extract output: {}", e))?;

            let (shape_ref, raw_data) = output_tensor;
            let s1 = shape_ref[1] as usize;
            let s2 = shape_ref[2] as usize;
            let s3 = shape_ref[3] as usize;

            // Accumulate with blending
            for ch in 0..s1.min(self.n_channels) {
                for f in 0..s2.min(dim_f) {
                    for t in 0..chunk_len.min(s3) {
                        let val = raw_data[ch * s2 * s3 + f * s3 + t];
                        let w = blend_window[t];
                        output_acc[ch][[f, start + t]] += val * w;
                    }
                }
            }
            for t in 0..chunk_len {
                weight_sum[start + t] += blend_window[t];
            }

            if let Some(cb) = &progress {
                cb((chunk_idx + 1) as f32 / n_chunks as f32);
            }
        }

        // Normalize by overlap weights
        for ch in 0..self.n_channels {
            for f in 0..dim_f {
                for t in 0..n_frames {
                    if weight_sum[t] > 1e-8 {
                        output_acc[ch][[f, t]] /= weight_sum[t];
                    }
                }
            }
        }

        // Reconstruct vocal spectrograms from model output
        // Output channels: [ch0_real, ch0_imag, ch1_real, ch1_imag]
        let mut vocal_channels: Vec<Vec<f32>> = Vec::new();
        let mut accomp_channels: Vec<Vec<f32>> = Vec::new();

        for stereo_ch in 0..2 {
            let re_idx = stereo_ch * 2;
            let im_idx = stereo_ch * 2 + 1;

            let mut vocal_spec = Array2::<Complex32>::zeros((freq_bins, n_frames));
            for f in 0..dim_f {
                for t in 0..n_frames {
                    vocal_spec[[f, t]] = Complex32::new(
                        output_acc[re_idx][[f, t]],
                        output_acc[im_idx][[f, t]],
                    );
                }
            }

            let vocals = istft(&vocal_spec, N_FFT, HOP_LENGTH, &self.window, sample_len);
            let accomp: Vec<f32> = stereo[stereo_ch]
                .iter()
                .zip(vocals.iter())
                .map(|(orig, voc)| orig - voc)
                .collect();

            vocal_channels.push(vocals);
            accomp_channels.push(accomp);
        }

        Ok((vocal_channels, accomp_channels))
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
