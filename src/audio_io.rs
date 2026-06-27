use std::path::Path;

use rubato::Resampler;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct DecodedAudio {
    pub samples: Vec<Vec<f32>>,
    pub sample_rate: u32,
    pub channels: usize,
}

pub fn decode_audio(path: &Path) -> Result<DecodedAudio, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("Unsupported audio format: {}", e))?;

    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or("No audio track found")?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("Unknown sample rate")?;

    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2);

    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Failed to create decoder: {}", e))?;

    let mut channel_buffers: Vec<Vec<f32>> = vec![Vec::new(); channels];

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => return Err(format!("Error reading packet: {}", e)),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("Decode error: {}", e)),
        };

        let spec = *decoded.spec();
        let num_frames = decoded.capacity();
        let mut sample_buf = SampleBuffer::<f32>::new(num_frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        let samples = sample_buf.samples();

        let ch = spec.channels.count();
        for (i, sample) in samples.iter().enumerate() {
            let channel_idx = i % ch;
            if channel_idx < channels {
                channel_buffers[channel_idx].push(*sample);
            }
        }
    }

    if channel_buffers.iter().all(|c| c.is_empty()) {
        return Err("No audio samples decoded".into());
    }

    Ok(DecodedAudio {
        samples: channel_buffers,
        sample_rate,
        channels,
    })
}

pub fn write_wav(path: &Path, samples: &[Vec<f32>], sample_rate: u32) -> Result<(), String> {
    let channels = samples.len() as u16;
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| format!("Failed to create WAV file: {}", e))?;

    let num_samples = samples.iter().map(|c| c.len()).min().unwrap_or(0);
    for i in 0..num_samples {
        for ch in samples {
            writer
                .write_sample(ch[i])
                .map_err(|e| format!("Failed to write sample: {}", e))?;
        }
    }

    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize WAV: {}", e))?;
    Ok(())
}

pub fn resample(
    samples: &[Vec<f32>],
    from_rate: u32,
    to_rate: u32,
) -> Result<Vec<Vec<f32>>, String> {
    if from_rate == to_rate {
        return Ok(samples.to_vec());
    }

    let channels = samples.len();
    let ratio = to_rate as f64 / from_rate as f64;
    let chunk_size = 1024;

    let params = rubato::SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        oversampling_factor: 128,
        interpolation: rubato::SincInterpolationType::Linear,
        window: rubato::WindowFunction::BlackmanHarris2,
    };

    let mut resampler = rubato::SincFixedIn::<f32>::new(
        ratio,
        2.0,
        params,
        chunk_size,
        channels,
    )
    .map_err(|e| format!("Failed to create resampler: {}", e))?;

    let input_len = samples[0].len();
    let mut output: Vec<Vec<f32>> = vec![Vec::new(); channels];
    let mut pos = 0;

    while pos < input_len {
        let end = (pos + chunk_size).min(input_len);
        let actual_len = end - pos;

        let chunk: Vec<Vec<f32>> = (0..channels)
            .map(|ch| {
                let mut c = samples[ch][pos..end].to_vec();
                if c.len() < chunk_size {
                    c.resize(chunk_size, 0.0);
                }
                c
            })
            .collect();

        let resampled = resampler
            .process(&chunk, None)
            .map_err(|e| format!("Resample error: {}", e))?;

        if actual_len < chunk_size {
            let expected_out = (actual_len as f64 * ratio).ceil() as usize;
            for (ch, data) in resampled.iter().enumerate() {
                let take = expected_out.min(data.len());
                output[ch].extend_from_slice(&data[..take]);
            }
        } else {
            for (ch, data) in resampled.iter().enumerate() {
                output[ch].extend_from_slice(data);
            }
        }

        pos = end;
    }

    Ok(output)
}
