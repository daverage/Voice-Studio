use anyhow::{anyhow, Context};
use std::convert::TryInto;
use std::io::{Cursor, Read};

const MAGIC: [u8; 4] = *b"DTLN";
const VERSION: u32 = 1;
const ENDIANNESS_LITTLE: u8 = 0;
const DTYPE_F32: u8 = 0;
const DTYPE_F16: u8 = 1;

pub static DTLN_WEIGHTS: &[u8] = include_bytes!("../../assets/dtln/dtln_weights_v1.bin");

struct TensorSpec {
    name: &'static str,
    shape: &'static [usize],
}

const TENSOR_ORDER: [TensorSpec; 20] = [
    TensorSpec {
        name: "lstm_4_W",
        shape: &[1, 512, 257],
    },
    TensorSpec {
        name: "lstm_4_R",
        shape: &[1, 512, 128],
    },
    TensorSpec {
        name: "lstm_4_B",
        shape: &[1, 1024],
    },
    TensorSpec {
        name: "lstm_5_W",
        shape: &[1, 512, 128],
    },
    TensorSpec {
        name: "lstm_5_R",
        shape: &[1, 512, 128],
    },
    TensorSpec {
        name: "lstm_5_B",
        shape: &[1, 1024],
    },
    TensorSpec {
        name: "dense_2/kernel:0",
        shape: &[128, 257],
    },
    TensorSpec {
        name: "dense_2/bias:0",
        shape: &[257],
    },
    TensorSpec {
        name: "conv1d_2/kernel:0",
        shape: &[256, 512, 1],
    },
    TensorSpec {
        name: "conv1d_3/kernel:0",
        shape: &[512, 256, 1],
    },
    TensorSpec {
        name: "model_2/instant_layer_normalization_1/mul/ReadVariableOp/resource:0",
        shape: &[256],
    },
    TensorSpec {
        name: "model_2/instant_layer_normalization_1/add_1/ReadVariableOp/resource:0",
        shape: &[256],
    },
    TensorSpec {
        name: "model_2/lstm_6/MatMul/ReadVariableOp/resource:0",
        shape: &[256, 512],
    },
    TensorSpec {
        name: "model_2/lstm_6/MatMul_1/ReadVariableOp/resource:0",
        shape: &[128, 512],
    },
    TensorSpec {
        name: "model_2/lstm_6/BiasAdd/ReadVariableOp/resource:0",
        shape: &[512],
    },
    TensorSpec {
        name: "model_2/lstm_7/MatMul/ReadVariableOp/resource:0",
        shape: &[128, 512],
    },
    TensorSpec {
        name: "model_2/lstm_7/MatMul_1/ReadVariableOp/resource:0",
        shape: &[128, 512],
    },
    TensorSpec {
        name: "model_2/lstm_7/BiasAdd/ReadVariableOp/resource:0",
        shape: &[512],
    },
    TensorSpec {
        name: "model_2/dense_3/Tensordot/Reshape_1:0",
        shape: &[128, 256],
    },
    TensorSpec {
        name: "model_2/dense_3/BiasAdd/ReadVariableOp/resource:0",
        shape: &[256],
    },
];

#[derive(Debug)]
struct TableEntry {
    name: String,
    shape: Vec<usize>,
    offset_bytes: usize,
    len_elements: usize,
}

/// Layout of the biases that accompany the LSTM layers.
enum BiasLayout {
    /// Separate kernels for W and R gates.
    Dual,
    /// Only the gate bias is provided (e.g., when the recurrent bias is baked in elsewhere).
    Single,
}

pub struct LstmLayer {
    pub w: Box<[f32]>,
    pub r: Box<[f32]>,
    pub bias_w: Box<[f32]>,
    pub bias_r: Box<[f32]>,
    pub input_size: usize,
    pub hidden_size: usize,
}

impl LstmLayer {
    fn new(
        w: Box<[f32]>,
        r: Box<[f32]>,
        bias: Box<[f32]>,
        layout: BiasLayout,
        input_size: usize,
    ) -> anyhow::Result<Self> {
        let total_gates = 4;
        if w.len() % (total_gates * input_size) != 0 {
            return Err(anyhow!("LSTM weight shape does not align with input size"));
        }
        let hidden_size = w.len() / (total_gates * input_size);
        if hidden_size == 0 {
            return Err(anyhow!("LSTM hidden size computed as zero"));
        }
        let expected_r_len = total_gates * hidden_size * hidden_size;
        if r.len() != expected_r_len {
            return Err(anyhow!("LSTM recurrent matrix has unexpected length"));
        }

        let gate_len = total_gates * hidden_size;
        match layout {
            BiasLayout::Dual => {
                if bias.len() != gate_len * 2 {
                    return Err(anyhow!("Dual-bias layout requires 2 * 4h entries"));
                }
                let bias_vec = bias.into_vec();
                let bias_w = bias_vec[..gate_len].to_vec().into_boxed_slice();
                let bias_r = bias_vec[gate_len..].to_vec().into_boxed_slice();
                Ok(Self {
                    w,
                    r,
                    bias_w,
                    bias_r,
                    input_size,
                    hidden_size,
                })
            }
            BiasLayout::Single => {
                if bias.len() != gate_len {
                    return Err(anyhow!("Single-bias layout requires 4h entries"));
                }
                let bias_r = vec![0.0f32; gate_len].into_boxed_slice();
                Ok(Self {
                    w,
                    r,
                    bias_w: bias,
                    bias_r,
                    input_size,
                    hidden_size,
                })
            }
        }
    }

    pub fn hidden_size(&self) -> usize {
        self.hidden_size
    }

    pub fn input_size(&self) -> usize {
        self.input_size
    }
}

/// Bag of raw tensors that power the native DTLN inference routine.
pub struct DtlnWeights {
    pub stage1_lstm4: LstmLayer,
    pub stage1_lstm5: LstmLayer,
    pub stage1_dense_kernel: Box<[f32]>,
    pub stage1_dense_bias: Box<[f32]>,
    pub stage2_conv2_kernel: Box<[f32]>,
    pub stage2_conv3_kernel: Box<[f32]>,
    pub stage2_norm_mul: Box<[f32]>,
    pub stage2_norm_add: Box<[f32]>,
    pub stage2_lstm6: LstmLayer,
    pub stage2_lstm7: LstmLayer,
    pub stage2_dense_kernel: Box<[f32]>,
    pub stage2_dense_bias: Box<[f32]>,
}

impl DtlnWeights {
    pub fn from_bytes(blob: &[u8]) -> anyhow::Result<Self> {
        let mut cursor = Cursor::new(blob);
        let mut magic = [0u8; 4];
        cursor.read_exact(&mut magic)?;
        if magic != MAGIC {
            return Err(anyhow!("DTLN weights magic mismatch"));
        }

        let version = read_u32(&mut cursor)?;
        if version != VERSION {
            return Err(anyhow!("Unsupported DTLN weights version {version}"));
        }

        let endian = read_u8(&mut cursor)?;
        if endian != ENDIANNESS_LITTLE {
            return Err(anyhow!("Only little-endian blobs supported"));
        }

        let dtype = read_u8(&mut cursor)?;
        if dtype != DTYPE_F32 && dtype != DTYPE_F16 {
            return Err(anyhow!("Unsupported tensor dtype {dtype}"));
        }

        let tensor_count = read_u32(&mut cursor)? as usize;
        if tensor_count != TENSOR_ORDER.len() {
            return Err(anyhow!(
                "Expected {} tensors, found {}",
                TENSOR_ORDER.len(),
                tensor_count
            ));
        }

        let mut entries = Vec::with_capacity(tensor_count);
        for _ in 0..tensor_count {
            entries.push(read_table_entry(&mut cursor)?);
        }

        let data_start = cursor.position() as usize;
        let data_section = blob
            .get(data_start..)
            .context("DTLN weights truncated after tensor table")?;

        let mut raw_tensors = Vec::with_capacity(tensor_count);
        for (spec, entry) in TENSOR_ORDER.iter().zip(entries.iter()) {
            if spec.name != entry.name {
                return Err(anyhow!(
                    "Tensor order mismatch: expected {}, saw {}",
                    spec.name,
                    entry.name
                ));
            }

            if spec.shape != entry.shape.as_slice() {
                return Err(anyhow!(
                    "Shape mismatch for {}: expected {:?}, got {:?}",
                    entry.name,
                    spec.shape,
                    entry.shape
                ));
            }

            let total_elems = shape_product(&entry.shape)
                .ok_or_else(|| anyhow!("Shape overflow for {}", entry.name))?;
            if total_elems != entry.len_elements {
                return Err(anyhow!(
                    "{} claims {} elements but table says {}",
                    entry.name,
                    total_elems,
                    entry.len_elements
                ));
            }

            let elem_bytes: usize = if dtype == DTYPE_F32 { 4 } else { 2 };
            let bytes_len = elem_bytes
                .checked_mul(entry.len_elements)
                .ok_or_else(|| anyhow!("Byte count overflow for {}", entry.name))?;
            let start = data_section
                .get(entry.offset_bytes..entry.offset_bytes + bytes_len)
                .with_context(|| format!("{} lies outside blob bounds", entry.name))?;

            raw_tensors.push(parse_tensor_data(dtype, start, entry.len_elements)?);
        }

        let converted = entries
            .into_iter()
            .zip(raw_tensors.into_iter())
            .map(|(entry, tensor)| convert_tensor(entry, tensor))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let mut iter = converted.into_iter();

        let stage1_lstm4 = LstmLayer::new(
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            BiasLayout::Dual,
            257,
        )?;
        let stage1_lstm5 = LstmLayer::new(
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            BiasLayout::Dual,
            128,
        )?;
        let stage1_dense_kernel = iter.next().unwrap();
        let stage1_dense_bias = iter.next().unwrap();
        let stage2_conv2_kernel = iter.next().unwrap();
        let stage2_conv3_kernel = iter.next().unwrap();
        let stage2_norm_mul = iter.next().unwrap();
        let stage2_norm_add = iter.next().unwrap();
        let stage2_lstm6 = LstmLayer::new(
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            BiasLayout::Single,
            256,
        )?;
        let stage2_lstm7 = LstmLayer::new(
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            BiasLayout::Single,
            128,
        )?;
        let stage2_dense_kernel = iter.next().unwrap();
        let stage2_dense_bias = iter.next().unwrap();

        Ok(Self {
            stage1_lstm4,
            stage1_lstm5,
            stage1_dense_kernel,
            stage1_dense_bias,
            stage2_conv2_kernel,
            stage2_conv3_kernel,
            stage2_norm_mul,
            stage2_norm_add,
            stage2_lstm6,
            stage2_lstm7,
            stage2_dense_kernel,
            stage2_dense_bias,
        })
    }
}

fn convert_tensor(entry: TableEntry, tensor: Box<[f32]>) -> anyhow::Result<Box<[f32]>> {
    match entry.name.as_str() {
        "conv1d_2/kernel:0" => {
            convert_conv_kernel(tensor, entry.shape[0], entry.shape[1], entry.shape[2])
        }
        "conv1d_3/kernel:0" => {
            convert_conv_kernel(tensor, entry.shape[0], entry.shape[1], entry.shape[2])
        }
        "model_2/lstm_6/MatMul/ReadVariableOp/resource:0" => {
            transpose_matrix(tensor, entry.shape[0], entry.shape[1])
        }
        "model_2/lstm_6/MatMul_1/ReadVariableOp/resource:0" => {
            transpose_matrix(tensor, entry.shape[0], entry.shape[1])
        }
        "model_2/lstm_7/MatMul/ReadVariableOp/resource:0" => {
            transpose_matrix(tensor, entry.shape[0], entry.shape[1])
        }
        "model_2/lstm_7/MatMul_1/ReadVariableOp/resource:0" => {
            transpose_matrix(tensor, entry.shape[0], entry.shape[1])
        }
        _ => Ok(tensor),
    }
}

fn convert_conv_kernel(
    tensor: Box<[f32]>,
    out_channels: usize,
    in_channels: usize,
    kernel_w: usize,
) -> anyhow::Result<Box<[f32]>> {
    if kernel_w != 1 {
        return Err(anyhow!("Only kernel size 1 supported"));
    }
    if tensor.len() != out_channels * in_channels * kernel_w {
        return Err(anyhow!("Conv kernel length mismatch"));
    }
    let data = tensor.into_vec();
    let mut reordered = Vec::with_capacity(out_channels * in_channels);
    for in_idx in 0..in_channels {
        for out_idx in 0..out_channels {
            reordered.push(data[out_idx * in_channels + in_idx]);
        }
    }
    Ok(reordered.into_boxed_slice())
}

fn transpose_matrix(tensor: Box<[f32]>, rows: usize, cols: usize) -> anyhow::Result<Box<[f32]>> {
    if tensor.len() != rows * cols {
        return Err(anyhow!("Matrix length mismatch for transpose"));
    }
    let mut buffer = Vec::with_capacity(rows * cols);
    for col in 0..cols {
        for row in 0..rows {
            buffer.push(tensor[row * cols + col]);
        }
    }
    Ok(buffer.into_boxed_slice())
}

fn shape_product(shape: &[usize]) -> Option<usize> {
    shape
        .iter()
        .copied()
        .try_fold(1usize, |acc, dim| acc.checked_mul(dim))
}

fn parse_tensor_data(dtype: u8, data: &[u8], len: usize) -> anyhow::Result<Box<[f32]>> {
    match dtype {
        DTYPE_F32 => {
            let mut vec = Vec::with_capacity(len);
            for chunk in data.chunks_exact(4) {
                let array: [u8; 4] = chunk.try_into().unwrap();
                vec.push(f32::from_le_bytes(array));
            }
            Ok(vec.into_boxed_slice())
        }
        DTYPE_F16 => {
            let mut vec = Vec::with_capacity(len);
            for chunk in data.chunks_exact(2) {
                let array: [u8; 2] = chunk.try_into().unwrap();
                let bits = u16::from_le_bytes(array);
                vec.push(half_to_f32(bits));
            }
            Ok(vec.into_boxed_slice())
        }
        other => Err(anyhow!("unsupported dtype {other}")),
    }
}

fn read_table_entry(cursor: &mut Cursor<&[u8]>) -> anyhow::Result<TableEntry> {
    let name_len = read_u16(cursor)? as usize;
    let mut name_bytes = vec![0u8; name_len];
    cursor.read_exact(&mut name_bytes)?;
    let name = String::from_utf8(name_bytes)?;

    let ndim = read_u8(cursor)? as usize;
    let mut shape = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        shape.push(read_u32(cursor)? as usize);
    }

    let offset_bytes = read_u32(cursor)? as usize;
    let len_elements = read_u32(cursor)? as usize;
    Ok(TableEntry {
        name,
        shape,
        offset_bytes,
        len_elements,
    })
}

fn read_u8(cursor: &mut Cursor<&[u8]>) -> anyhow::Result<u8> {
    let mut buf = [0u8; 1];
    cursor.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn read_u16(cursor: &mut Cursor<&[u8]>) -> anyhow::Result<u16> {
    let mut buf = [0u8; 2];
    cursor.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> anyhow::Result<u32> {
    let mut buf = [0u8; 4];
    cursor.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn half_to_f32(bits: u16) -> f32 {
    const SIGN_MASK: u16 = 0x8000;
    const EXP_MASK: u16 = 0x7C00;
    const FRAC_MASK: u16 = 0x03FF;

    let sign = ((bits & SIGN_MASK) as u32) << 16;
    let exp = (bits & EXP_MASK) >> 10;
    let frac = bits & FRAC_MASK;

    let bits32 = match exp {
        0 => {
            if frac == 0 {
                sign
            } else {
                let mut mant = frac as u32;
                let mut e = -14;
                while mant & 0x400 == 0 {
                    mant <<= 1;
                    e -= 1;
                }
                mant &= FRAC_MASK as u32;
                let exp_bits = ((e + 127) as u32) << 23;
                let mant_bits = mant << 13;
                sign | exp_bits | mant_bits
            }
        }
        0x1F => {
            let mant_bits = (frac as u32) << 13;
            sign | 0x7F800000 | mant_bits
        }
        _ => {
            let exp_bits = ((exp as i32 - 15 + 127) as u32) << 23;
            let mant_bits = (frac as u32) << 13;
            sign | exp_bits | mant_bits
        }
    };

    f32::from_bits(bits32)
}
