//! Strict Safetensors checkpoint loader with loud shape and dtype validation.
//! Ingests BF16 weights, decodes to FP32 for oracle evaluation, and preserves raw BF16 bytes for acceleration.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// Loud error enum describing checkpoint parsing and validation failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointError {
    /// A required tensor in the model catalog was not found in the checkpoint.
    MissingTensor(String),
    /// A tensor's shape in the checkpoint does not match the canonical model architecture.
    ShapeMismatch {
        tensor: String,
        expected: Vec<usize>,
        actual: Vec<usize>,
    },
    /// A tensor's dtype is not BF16.
    DtypeMismatch {
        tensor: String,
        expected: String,
        actual: String,
    },
    /// A tensor's byte range exceeds the binary payload or does not match shape element count.
    OffsetOutOfBounds {
        tensor: String,
        offset: usize,
        buffer_len: usize,
    },
    /// Safetensors binary header is malformed, truncated, or invalid JSON.
    InvalidHeader(String),
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTensor(name) => {
                write!(f, "Missing required tensor in checkpoint: {}", name)
            }
            Self::ShapeMismatch {
                tensor,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Shape mismatch for tensor {}: expected {:?}, got {:?}",
                    tensor, expected, actual
                )
            }
            Self::DtypeMismatch {
                tensor,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Dtype mismatch for tensor {}: expected {}, got {}",
                    tensor, expected, actual
                )
            }
            Self::OffsetOutOfBounds {
                tensor,
                offset,
                buffer_len,
            } => {
                write!(
                    f,
                    "Offset out of bounds for tensor {}: offset {} exceeds buffer length {}",
                    tensor, offset, buffer_len
                )
            }
            Self::InvalidHeader(err) => {
                write!(f, "Invalid safetensors header: {}", err)
            }
        }
    }
}

impl std::error::Error for CheckpointError {}

/// Dual storage representation of loaded model weights:
/// FP32 floats for the reference correctness oracle and raw BF16 bytes for hardware kernels.
#[derive(Debug, Clone, Default)]
pub struct LoadedCheckpoint {
    pub fp32_weights: HashMap<String, Vec<f32>>,
    pub raw_bf16_weights: HashMap<String, Vec<u8>>,
    pub shapes: HashMap<String, Vec<usize>>,
}

impl LoadedCheckpoint {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tensor_count(&self) -> usize {
        self.fp32_weights.len()
    }

    pub fn contains_tensor(&self, name: &str) -> bool {
        self.fp32_weights.contains_key(name)
    }

    pub fn get_fp32(&self, name: &str) -> Option<&Vec<f32>> {
        self.fp32_weights.get(name)
    }

    pub fn get_raw_bf16(&self, name: &str) -> Option<&Vec<u8>> {
        self.raw_bf16_weights.get(name)
    }

    pub fn get_shape(&self, name: &str) -> Option<&Vec<usize>> {
        self.shapes.get(name)
    }
}

/// Decodes little-endian bfloat16 raw bytes to single-precision f32.
/// Each 16-bit BF16 word occupies the upper 16 bits of the IEEE-754 32-bit float.
pub fn decode_bf16_to_fp32(bf16_bytes: &[u8]) -> Vec<f32> {
    let count = bf16_bytes.len() / 2;
    let mut out = Vec::with_capacity(count);
    for chunk in bf16_bytes.chunks_exact(2) {
        let bits = u16::from_le_bytes([chunk[0], chunk[1]]);
        out.push(f32::from_bits((bits as u32) << 16));
    }
    out
}

/// Encodes single-precision f32 to little-endian bfloat16 raw bytes by truncation of lower 16 bits.
pub fn encode_fp32_to_bf16(floats: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(floats.len() * 2);
    for &val in floats {
        let bits = val.to_bits();
        let bf16_bits = (bits >> 16) as u16;
        out.extend_from_slice(&bf16_bits.to_le_bytes());
    }
    out
}

/// Generates the canonical catalog of all 201 TinyLlama-1.1B tensors with their expected shapes.
pub fn tinyllama_catalog() -> Vec<(String, Vec<usize>)> {
    let mut catalog = Vec::with_capacity(201);

    catalog.push(("model.embed_tokens.weight".to_string(), vec![32000, 2048]));

    for layer in 0..22 {
        let prefix = format!("model.layers.{}", layer);
        catalog.push((format!("{}.input_layernorm.weight", prefix), vec![2048]));
        catalog.push((
            format!("{}.self_attn.q_proj.weight", prefix),
            vec![2048, 2048],
        ));
        catalog.push((
            format!("{}.self_attn.k_proj.weight", prefix),
            vec![256, 2048],
        ));
        catalog.push((
            format!("{}.self_attn.v_proj.weight", prefix),
            vec![256, 2048],
        ));
        catalog.push((
            format!("{}.self_attn.o_proj.weight", prefix),
            vec![2048, 2048],
        ));
        catalog.push((
            format!("{}.post_attention_layernorm.weight", prefix),
            vec![2048],
        ));
        catalog.push((
            format!("{}.mlp.gate_proj.weight", prefix),
            vec![5632, 2048],
        ));
        catalog.push((format!("{}.mlp.up_proj.weight", prefix), vec![5632, 2048]));
        catalog.push((
            format!("{}.mlp.down_proj.weight", prefix),
            vec![2048, 5632],
        ));
    }

    catalog.push(("model.norm.weight".to_string(), vec![2048]));
    catalog.push(("lm_head.weight".to_string(), vec![32000, 2048]));

    catalog
}

/// Parses a safetensors binary buffer and strictly validates against a specified tensor catalog.
pub fn parse_safetensors_with_catalog(
    bytes: &[u8],
    catalog: &[(String, Vec<usize>)],
) -> Result<LoadedCheckpoint, CheckpointError> {
    if bytes.len() < 8 {
        return Err(CheckpointError::InvalidHeader(
            "Buffer smaller than 8-byte header prefix".to_string(),
        ));
    }

    let header_len_raw = u64::from_le_bytes(
        bytes[0..8]
            .try_into()
            .map_err(|_| CheckpointError::InvalidHeader("Failed to read header size".to_string()))?,
    );
    let header_len = usize::try_from(header_len_raw)
        .map_err(|_| CheckpointError::InvalidHeader("Header length exceeds address space".to_string()))?;

    let header_end = 8usize
        .checked_add(header_len)
        .ok_or_else(|| CheckpointError::InvalidHeader("Header length arithmetic overflow".to_string()))?;

    if bytes.len() < header_end {
        return Err(CheckpointError::InvalidHeader(format!(
            "Header length {} exceeds total buffer size {}",
            header_len,
            bytes.len()
        )));
    }

    let header_str = std::str::from_utf8(&bytes[8..header_end]).map_err(|e| {
        CheckpointError::InvalidHeader(format!("Header JSON is not valid UTF-8: {}", e))
    })?;

    let header: serde_json::Value = serde_json::from_str(header_str).map_err(|e| {
        CheckpointError::InvalidHeader(format!("Failed to parse header JSON: {}", e))
    })?;

    let header_obj = header.as_object().ok_or_else(|| {
        CheckpointError::InvalidHeader("Header JSON must be an object".to_string())
    })?;

    let data_bytes = &bytes[header_end..];

    let mut fp32_weights = HashMap::with_capacity(catalog.len());
    let mut raw_bf16_weights = HashMap::with_capacity(catalog.len());
    let mut shapes = HashMap::with_capacity(catalog.len());

    for (name, expected_shape) in catalog {
        let info = header_obj
            .get(name)
            .ok_or_else(|| CheckpointError::MissingTensor(name.clone()))?;

        // 1. Verify dtype is BF16
        let dtype = info
            .get("dtype")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CheckpointError::MissingTensor(format!("{}.dtype", name)))?;

        if dtype != "BF16" {
            return Err(CheckpointError::DtypeMismatch {
                tensor: name.clone(),
                expected: "BF16".to_string(),
                actual: dtype.to_string(),
            });
        }

        // 2. Verify shape matches canonical architecture
        let shape_arr = info
            .get("shape")
            .and_then(|v| v.as_array())
            .ok_or_else(|| CheckpointError::MissingTensor(format!("{}.shape", name)))?;

        let actual_shape: Vec<usize> = shape_arr
            .iter()
            .map(|v| {
                v.as_u64().map(|n| n as usize).ok_or_else(|| {
                    CheckpointError::InvalidHeader(format!(
                        "Invalid dimension in shape for {}",
                        name
                    ))
                })
            })
            .collect::<Result<Vec<usize>, CheckpointError>>()?;

        if actual_shape != *expected_shape {
            return Err(CheckpointError::ShapeMismatch {
                tensor: name.clone(),
                expected: expected_shape.clone(),
                actual: actual_shape,
            });
        }

        // 3. Verify offsets and byte length
        let offsets_arr = info
            .get("data_offsets")
            .and_then(|v| v.as_array())
            .ok_or_else(|| CheckpointError::MissingTensor(format!("{}.data_offsets", name)))?;

        if offsets_arr.len() != 2 {
            return Err(CheckpointError::InvalidHeader(format!(
                "data_offsets for {} must have exactly 2 elements",
                name
            )));
        }

        let start = offsets_arr[0].as_u64().ok_or_else(|| {
            CheckpointError::InvalidHeader(format!("Invalid start offset for {}", name))
        })? as usize;

        let end = offsets_arr[1].as_u64().ok_or_else(|| {
            CheckpointError::InvalidHeader(format!("Invalid end offset for {}", name))
        })? as usize;

        let element_count: usize = expected_shape.iter().product();
        let expected_bytes = element_count * 2; // BF16 is 2 bytes per element

        if start > end || (end - start) != expected_bytes || end > data_bytes.len() {
            return Err(CheckpointError::OffsetOutOfBounds {
                tensor: name.clone(),
                offset: end,
                buffer_len: data_bytes.len(),
            });
        }

        // 4. Ingest raw BF16 bytes and decode to FP32 in-memory
        let raw_slice = &data_bytes[start..end];
        let fp32_slice = decode_bf16_to_fp32(raw_slice);

        raw_bf16_weights.insert(name.clone(), raw_slice.to_vec());
        fp32_weights.insert(name.clone(), fp32_slice);
        shapes.insert(name.clone(), actual_shape);
    }

    Ok(LoadedCheckpoint {
        fp32_weights,
        raw_bf16_weights,
        shapes,
    })
}

/// Loads a TinyLlama safetensors checkpoint directly from a binary byte buffer.
pub fn load_safetensors_from_bytes(bytes: &[u8]) -> Result<LoadedCheckpoint, CheckpointError> {
    let catalog = tinyllama_catalog();
    parse_safetensors_with_catalog(bytes, &catalog)
}

/// Loads and validates a TinyLlama safetensors checkpoint from a filesystem path.
pub fn load_safetensors_checkpoint<P: AsRef<Path>>(path: P) -> Result<LoadedCheckpoint, CheckpointError> {
    let p = path.as_ref();
    let bytes = std::fs::read(p).map_err(|e| {
        CheckpointError::InvalidHeader(format!("Failed to read checkpoint at {}: {}", p.display(), e))
    })?;
    load_safetensors_from_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constructs a minimal valid safetensors binary buffer for testing.
    fn build_test_safetensors(
        tensors: &[(&str, &str, &[usize], &[u8])],
    ) -> Vec<u8> {
        let mut header_map = serde_json::Map::new();
        let mut data_payload = Vec::new();

        for (name, dtype, shape, payload) in tensors {
            let start = data_payload.len();
            data_payload.extend_from_slice(payload);
            let end = data_payload.len();

            let mut tensor_info = serde_json::Map::new();
            tensor_info.insert("dtype".to_string(), serde_json::Value::String(dtype.to_string()));
            let shape_val = shape
                .iter()
                .map(|&d| serde_json::Value::Number(serde_json::Number::from(d)))
                .collect();
            tensor_info.insert("shape".to_string(), serde_json::Value::Array(shape_val));
            let offsets_val = vec![
                serde_json::Value::Number(serde_json::Number::from(start)),
                serde_json::Value::Number(serde_json::Number::from(end)),
            ];
            tensor_info.insert(
                "data_offsets".to_string(),
                serde_json::Value::Array(offsets_val),
            );

            header_map.insert(name.to_string(), serde_json::Value::Object(tensor_info));
        }

        let header_json = serde_json::Value::Object(header_map).to_string();
        let header_bytes = header_json.as_bytes();
        let header_len = header_bytes.len() as u64;

        let mut buffer = Vec::new();
        buffer.extend_from_slice(&header_len.to_le_bytes());
        buffer.extend_from_slice(header_bytes);
        buffer.extend_from_slice(&data_payload);
        buffer
    }

    #[test]
    fn test_tinyllama_catalog_size_and_shapes() {
        let catalog = tinyllama_catalog();
        assert_eq!(catalog.len(), 201);

        assert_eq!(catalog[0].0, "model.embed_tokens.weight");
        assert_eq!(catalog[0].1, vec![32000, 2048]);

        assert_eq!(catalog[200].0, "lm_head.weight");
        assert_eq!(catalog[200].1, vec![32000, 2048]);

        assert_eq!(catalog[199].0, "model.norm.weight");
        assert_eq!(catalog[199].1, vec![2048]);
    }

    #[test]
    fn test_bf16_fp32_codec_roundtrip() {
        let floats = vec![0.0f32, 1.0f32, -1.0f32, 0.5f32, 64.0f32];
        let bf16_bytes = encode_fp32_to_bf16(&floats);
        let decoded = decode_bf16_to_fp32(&bf16_bytes);

        assert_eq!(decoded.len(), floats.len());
        assert_eq!(decoded[0], 0.0);
        assert_eq!(decoded[1], 1.0);
        assert_eq!(decoded[2], -1.0);
        assert_eq!(decoded[3], 0.5);
        assert_eq!(decoded[4], 64.0);
    }

    #[test]
    fn test_valid_custom_catalog_parsing() {
        let floats = vec![1.5f32, 2.5f32, -0.5f32, 4.0f32];
        let bf16_bytes = encode_fp32_to_bf16(&floats);
        let buffer = build_test_safetensors(&[("test.weight", "BF16", &[2, 2], &bf16_bytes)]);

        let catalog = vec![("test.weight".to_string(), vec![2, 2])];
        let loaded = parse_safetensors_with_catalog(&buffer, &catalog).unwrap();

        assert_eq!(loaded.tensor_count(), 1);
        assert_eq!(loaded.get_shape("test.weight").unwrap(), &vec![2, 2]);
        let fp32 = loaded.get_fp32("test.weight").unwrap();
        assert_eq!(fp32.len(), 4);
        assert_eq!(fp32[0], 1.5);
        assert_eq!(fp32[1], 2.5);
        assert_eq!(loaded.get_raw_bf16("test.weight").unwrap(), &bf16_bytes);
    }

    #[test]
    fn test_missing_tensor_loud_error() {
        let floats = vec![1.0f32, 2.0f32];
        let bf16_bytes = encode_fp32_to_bf16(&floats);
        let buffer = build_test_safetensors(&[("present.weight", "BF16", &[2], &bf16_bytes)]);

        let catalog = vec![
            ("present.weight".to_string(), vec![2]),
            ("missing.weight".to_string(), vec![2]),
        ];

        let err = parse_safetensors_with_catalog(&buffer, &catalog).unwrap_err();
        assert_eq!(err, CheckpointError::MissingTensor("missing.weight".to_string()));
    }

    #[test]
    fn test_shape_mismatch_loud_error() {
        let floats = vec![1.0f32, 2.0f32];
        let bf16_bytes = encode_fp32_to_bf16(&floats);
        let buffer = build_test_safetensors(&[("layer.weight", "BF16", &[2], &bf16_bytes)]);

        let catalog = vec![("layer.weight".to_string(), vec![4])];

        let err = parse_safetensors_with_catalog(&buffer, &catalog).unwrap_err();
        match err {
            CheckpointError::ShapeMismatch { tensor, expected, actual } => {
                assert_eq!(tensor, "layer.weight");
                assert_eq!(expected, vec![4]);
                assert_eq!(actual, vec![2]);
            }
            _ => panic!("Expected ShapeMismatch"),
        }
    }

    #[test]
    fn test_dtype_mismatch_loud_error() {
        let raw_f32_bytes = vec![0u8; 16]; // 4 floats
        let buffer = build_test_safetensors(&[("layer.weight", "F32", &[4], &raw_f32_bytes)]);

        let catalog = vec![("layer.weight".to_string(), vec![4])];

        let err = parse_safetensors_with_catalog(&buffer, &catalog).unwrap_err();
        match err {
            CheckpointError::DtypeMismatch { tensor, expected, actual } => {
                assert_eq!(tensor, "layer.weight");
                assert_eq!(expected, "BF16");
                assert_eq!(actual, "F32");
            }
            _ => panic!("Expected DtypeMismatch"),
        }
    }

    #[test]
    fn test_offset_out_of_bounds_loud_error() {
        // Create corrupted header where end offset exceeds payload
        let header_json = r#"{"weight":{"dtype":"BF16","shape":[4],"data_offsets":[0,100]}}"#;
        let header_bytes = header_json.as_bytes();
        let header_len = header_bytes.len() as u64;

        let mut buffer = Vec::new();
        buffer.extend_from_slice(&header_len.to_le_bytes());
        buffer.extend_from_slice(header_bytes);
        buffer.extend_from_slice(&[0u8; 8]); // only 8 bytes payload, offset requires 100

        let catalog = vec![("weight".to_string(), vec![4])];
        let err = parse_safetensors_with_catalog(&buffer, &catalog).unwrap_err();
        match err {
            CheckpointError::OffsetOutOfBounds { tensor, offset, buffer_len } => {
                assert_eq!(tensor, "weight");
                assert_eq!(offset, 100);
                assert_eq!(buffer_len, 8);
            }
            _ => panic!("Expected OffsetOutOfBounds"),
        }
    }

    #[test]
    fn test_invalid_header_truncated_buffer() {
        let buffer = vec![1, 2, 3];
        let catalog = vec![("weight".to_string(), vec![2])];
        let err = parse_safetensors_with_catalog(&buffer, &catalog).unwrap_err();
        match err {
            CheckpointError::InvalidHeader(_) => {}
            _ => panic!("Expected InvalidHeader"),
        }
    }

    #[test]
    fn test_header_len_arithmetic_overflow() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&u64::MAX.to_le_bytes());
        buffer.extend_from_slice(b"payload");
        let catalog = vec![("weight".to_string(), vec![2])];
        let err = parse_safetensors_with_catalog(&buffer, &catalog).unwrap_err();
        match err {
            CheckpointError::InvalidHeader(_) => {}
            _ => panic!("Expected InvalidHeader on arithmetic overflow"),
        }
    }
}
