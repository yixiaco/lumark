//! LZH lh4/lh5/lh7 可续写流式编码器。
//!
//! 上游 `oxiarc-lzhuf` 的块编码是标准的“命令数 + 三张 Huffman 表 + 命令流”，
//! 但公开的 `LzhEncoder::encode` 每次调用都会新建位写入器，无法跨块保持半字节
//! 状态。本模块复用其 LZSS 匹配器和块编码算法，由一个长期存在的
//! `MsbBitWriter` 连续写块，因此可以在任意大小输入上边读边压。
//!
//! 块编码与 Huffman 表生成逻辑改编自 `oxiarc-lzhuf 0.3.6`（Apache-2.0）。

use std::io::Write;

use oxiarc_core::MsbBitWriter;
use oxiarc_core::error::{OxiArcError, Result as OxiResult};
use oxiarc_lzhuf::lzss::{LzssEncoder, LzssToken};
use oxiarc_lzhuf::methods::LzhMethod;
use oxiarc_lzhuf::methods::constants::{NC, NT};

use crate::error::ArchiveError;

/// 码表最大码长。
const MAX_CODE_LEN: usize = 16;
/// 单个块最多包含的命令数；16 位命令计数字段的上限为 65535。
const MAX_COMMANDS_PER_BLOCK: usize = 0x4000;
/// 临时码表计数字段宽度。
const TEMP_CODE_BITS: u8 = 5;
/// 临时码表最大符号数。
const MAX_TEMP_CODES: usize = (1 << TEMP_CODE_BITS) - 1;
/// 辅助码表的最大码长。
const AUX_MAX_CODE_LEN: usize = 7;

/// 可跨多次 `write_chunk` 调用的 LZH 块编码器。
pub(crate) struct LzhStreamEncoder<'a, W: Write> {
    /// 压缩方法，决定窗口和偏移码表宽度。
    method: LzhMethod,
    /// 跨块保留历史窗口的 LZSS 匹配器。
    lzss: LzssEncoder,
    /// 跨块保留半字节状态的位写入器。
    bits: MsbBitWriter<&'a mut W>,
}

impl<'a, W: Write> LzhStreamEncoder<'a, W> {
    /// 为指定方法和输出创建流式编码器。
    pub(crate) fn new(writer: &'a mut W, method: LzhMethod) -> Self {
        let window = method.window_size().max(256);
        Self {
            method,
            lzss: LzssEncoder::new(window, method.min_match().max(3), method.max_match().max(3)),
            bits: MsbBitWriter::new(writer),
        }
    }

    /// 编码一段输入；块边界可以落在任意字节位置。
    pub(crate) fn write_chunk(&mut self, data: &[u8]) -> Result<(), ArchiveError> {
        if data.is_empty() {
            return Ok(());
        }
        let tokens = self.lzss.encode(data);
        self.encode_tokens(&tokens)
            .map_err(|error| ArchiveError::Corrupt(format!("LZH 编码失败：{error}")))
    }

    /// 刷新最后一个不完整字节。
    pub(crate) fn finish(&mut self) -> Result<(), ArchiveError> {
        self.bits.flush().map_err(ArchiveError::Io)
    }

    /// 已写出的压缩字节数（含当前尚未刷新的位）。
    pub(crate) fn output_bytes(&self) -> u64 {
        self.bits.bits_written().div_ceil(8)
    }

    /// 把一次 LZSS 输出按命令数切块并写出。
    fn encode_tokens(&mut self, tokens: &[LzssToken]) -> OxiResult<()> {
        if tokens.is_empty() {
            return Ok(());
        }
        let offset_bits = offset_bits(self.method);
        let max_offset_codes = max_offset_codes(self.method);
        for block in tokens.chunks(MAX_COMMANDS_PER_BLOCK) {
            encode_block(block, &mut self.bits, offset_bits, max_offset_codes)?;
        }
        Ok(())
    }
}

/// 方法对应的偏移树计数字段宽度。
fn offset_bits(method: LzhMethod) -> u8 {
    match method {
        LzhMethod::Lh4 | LzhMethod::Lh5 => 4,
        LzhMethod::Lh6 | LzhMethod::Lh7 => 5,
        _ => 0,
    }
}

/// 方法对应的最大偏移码数量。
fn max_offset_codes(method: LzhMethod) -> usize {
    (1usize << offset_bits(method)).saturating_sub(1)
}

/// 编码一个命令块：命令数、三张 Huffman 表和命令流。
fn encode_block<W: Write>(
    tokens: &[LzssToken],
    writer: &mut MsbBitWriter<W>,
    offset_bits: u8,
    max_offset_codes: usize,
) -> OxiResult<()> {
    debug_assert!(!tokens.is_empty(), "块必须至少包含一个命令");
    let mut c_freq = vec![0u32; NC];
    let mut p_freq = vec![0u32; max_offset_codes.max(1)];
    for token in tokens {
        match token {
            LzssToken::Literal(value) => c_freq[*value as usize] += 1,
            LzssToken::Match { length, distance } => {
                c_freq[length_to_csym(*length)] += 1;
                let (symbol, _, _) = encode_offset(*distance);
                if (symbol as usize) < p_freq.len() {
                    p_freq[symbol as usize] += 1;
                }
            }
        }
    }
    let c_lengths = build_code_lengths(&c_freq, MAX_CODE_LEN);
    let p_lengths = build_code_lengths(&p_freq, MAX_CODE_LEN);
    let c_codes = build_codes(&c_lengths);
    let p_codes = build_codes(&p_lengths);
    writer.put_bits(16, tokens.len() as u32)?;
    let c_degenerate = write_code_table(writer, &c_lengths)?;
    let p_degenerate = write_offset_table(writer, &p_lengths, offset_bits)?;
    for token in tokens {
        match token {
            LzssToken::Literal(value) => {
                let symbol = *value as usize;
                if !c_degenerate {
                    writer.put_bits(c_lengths[symbol], c_codes[symbol])?;
                }
            }
            LzssToken::Match { length, distance } => {
                let csym = length_to_csym(*length);
                if !c_degenerate {
                    writer.put_bits(c_lengths[csym], c_codes[csym])?;
                }
                let (symbol, extra_bits, extra_value) = encode_offset(*distance);
                let symbol = symbol as usize;
                if symbol >= p_lengths.len() {
                    return Err(OxiArcError::encoding_error(format!(
                        "偏移符号 {symbol} 超出码表大小 {}（距离 {distance}）",
                        p_lengths.len()
                    )));
                }
                if !p_degenerate {
                    writer.put_bits(p_lengths[symbol], p_codes[symbol])?;
                }
                if extra_bits > 0 {
                    writer.put_bits(extra_bits, extra_value)?;
                }
            }
        }
    }
    Ok(())
}

/// 把匹配长度映射为字符树符号。
fn length_to_csym(length: u16) -> usize {
    ((length as usize).saturating_sub(3) + 256).min(NC - 1)
}

/// 把 1 基匹配距离映射为偏移树符号和额外位。
fn encode_offset(distance: u16) -> (u8, u8, u32) {
    let offset = u32::from(distance) - 1;
    if offset == 0 {
        (0, 0, 0)
    } else if offset == 1 {
        (1, 0, 0)
    } else {
        let bits = 32 - offset.leading_zeros();
        (bits as u8, (bits - 1) as u8, offset - (1u32 << (bits - 1)))
    }
}

/// 写入一个原始码长值。
fn write_length_value<W: Write>(writer: &mut MsbBitWriter<W>, len: u8) -> OxiResult<()> {
    if len < 7 {
        writer.put_bits(3, u32::from(len))?;
    } else {
        writer.put_bits(3, 7)?;
        for _ in 0..(len - 7) {
            writer.put_bit(true)?;
        }
        writer.put_bit(false)?;
    }
    Ok(())
}

/// 写入不会被读取的退化临时码表。
fn write_temp_dummy<W: Write>(writer: &mut MsbBitWriter<W>) -> OxiResult<()> {
    writer.put_bits(TEMP_CODE_BITS, 0)?;
    writer.put_bits(TEMP_CODE_BITS, 0)?;
    Ok(())
}

/// 写入固定到单个符号的退化临时码表。
fn write_temp_single<W: Write>(writer: &mut MsbBitWriter<W>, symbol: u16) -> OxiResult<()> {
    writer.put_bits(TEMP_CODE_BITS, 0)?;
    writer.put_bits(TEMP_CODE_BITS, u32::from(symbol))?;
    Ok(())
}

/// 写入完整临时码表。
fn write_temp_full<W: Write>(writer: &mut MsbBitWriter<W>, lengths: &[u8]) -> OxiResult<()> {
    let count = lengths
        .iter()
        .rposition(|value| *value > 0)
        .map_or(0, |index| index + 1)
        .min(MAX_TEMP_CODES);
    writer.put_bits(TEMP_CODE_BITS, count as u32)?;
    for (index, value) in lengths.iter().take(count).enumerate() {
        write_length_value(writer, *value)?;
        if index == 2 {
            writer.put_bits(2, 0)?;
        }
    }
    Ok(())
}

/// 码表长度列表的传输指令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TempOp {
    /// 一个零长度位置。
    ZeroRun1,
    /// 3..=18 个零长度位置。
    ZeroRunShort(u8),
    /// 20..=531 个零长度位置。
    ZeroRunLong(u16),
    /// 单个非零码长。
    Length(u8),
}

impl TempOp {
    /// 指令对应的临时树符号。
    fn symbol(self) -> u16 {
        match self {
            Self::ZeroRun1 => 0,
            Self::ZeroRunShort(_) => 1,
            Self::ZeroRunLong(_) => 2,
            Self::Length(length) => u16::from(length) + 2,
        }
    }
}

/// 把码长数组拆成零游程和单长度指令。
fn c_length_program(lengths: &[u8], count: usize) -> Vec<TempOp> {
    let mut ops = Vec::new();
    let mut index = 0;
    while index < count {
        if lengths[index] == 0 {
            let mut run = 1;
            while index + run < count && lengths[index + run] == 0 {
                run += 1;
            }
            let mut remaining = run;
            while remaining > 0 {
                if remaining >= 20 {
                    let take = remaining.min(531);
                    ops.push(TempOp::ZeroRunLong((take - 20) as u16));
                    remaining -= take;
                } else if remaining >= 3 {
                    let take = remaining.min(18);
                    ops.push(TempOp::ZeroRunShort((take - 3) as u8));
                    remaining -= take;
                } else {
                    ops.push(TempOp::ZeroRun1);
                    remaining -= 1;
                }
            }
            index += run;
        } else {
            ops.push(TempOp::Length(lengths[index]));
            index += 1;
        }
    }
    ops
}

/// 统计临时树符号频率。
fn build_pt_freq(ops: &[TempOp]) -> Vec<u32> {
    let mut freq = vec![0u32; NT];
    for op in ops {
        let symbol = op.symbol() as usize;
        if symbol < NT {
            freq[symbol] += 1;
        }
    }
    freq
}

/// 写入字符树；返回是否使用零位退化码表。
fn write_code_table<W: Write>(writer: &mut MsbBitWriter<W>, lengths: &[u8]) -> OxiResult<bool> {
    let count = lengths
        .iter()
        .rposition(|value| *value > 0)
        .map_or(0, |index| index + 1);
    let used = lengths.iter().filter(|value| **value > 0).count();
    if count == 0 {
        write_temp_dummy(writer)?;
        writer.put_bits(9, 0)?;
        writer.put_bits(9, 0)?;
        return Ok(true);
    }
    if used == 1 {
        let symbol = lengths.iter().position(|value| *value > 0).unwrap_or(0);
        write_temp_dummy(writer)?;
        writer.put_bits(9, 0)?;
        writer.put_bits(9, symbol as u32)?;
        return Ok(true);
    }
    let ops = c_length_program(lengths, count);
    let pt_freq = build_pt_freq(&ops);
    let pt_lengths = build_code_lengths(&pt_freq, AUX_MAX_CODE_LEN);
    let pt_used = pt_freq.iter().filter(|value| **value > 0).count();
    if pt_used <= 1 {
        let symbol = pt_freq.iter().position(|value| *value > 0).unwrap_or(0);
        write_temp_single(writer, symbol as u16)?;
        writer.put_bits(9, count as u32)?;
        return Ok(false);
    }
    let pt_codes = build_codes(&pt_lengths);
    write_temp_full(writer, &pt_lengths)?;
    writer.put_bits(9, count as u32)?;
    for op in &ops {
        let symbol = op.symbol() as usize;
        writer.put_bits(pt_lengths[symbol], pt_codes[symbol])?;
        match *op {
            TempOp::ZeroRunShort(extra) => writer.put_bits(4, u32::from(extra))?,
            TempOp::ZeroRunLong(extra) => writer.put_bits(9, u32::from(extra))?,
            TempOp::ZeroRun1 | TempOp::Length(_) => {}
        }
    }
    Ok(false)
}

/// 写入偏移树；返回是否使用零位退化码表。
fn write_offset_table<W: Write>(
    writer: &mut MsbBitWriter<W>,
    lengths: &[u8],
    offset_bits: u8,
) -> OxiResult<bool> {
    let count = lengths
        .iter()
        .rposition(|value| *value > 0)
        .map_or(0, |index| index + 1);
    let used = lengths.iter().filter(|value| **value > 0).count();
    if count == 0 {
        writer.put_bits(offset_bits, 0)?;
        writer.put_bits(offset_bits, 0)?;
        return Ok(true);
    }
    if used == 1 {
        let symbol = lengths.iter().position(|value| *value > 0).unwrap_or(0);
        writer.put_bits(offset_bits, 0)?;
        writer.put_bits(offset_bits, symbol as u32)?;
        return Ok(true);
    }
    writer.put_bits(offset_bits, count as u32)?;
    for value in lengths.iter().take(count) {
        write_length_value(writer, *value)?;
    }
    Ok(false)
}

/// 用贪心 Huffman 合并生成码长，再限制到最大码长。
fn build_code_lengths(freqs: &[u32], max_len: usize) -> Vec<u8> {
    let mut lengths = vec![0u8; freqs.len()];
    let symbols: Vec<(usize, u32)> = freqs
        .iter()
        .enumerate()
        .filter(|(_, freq)| **freq > 0)
        .map(|(index, freq)| (index, *freq))
        .collect();
    if symbols.is_empty() {
        return lengths;
    }
    if symbols.len() == 1 {
        lengths[symbols[0].0] = 1;
        return lengths;
    }
    if symbols.len() == 2 {
        lengths[symbols[0].0] = 1;
        lengths[symbols[1].0] = 1;
        return lengths;
    }
    let mut nodes: Vec<(u64, Vec<usize>)> = symbols
        .iter()
        .map(|(symbol, freq)| (u64::from(*freq), vec![*symbol]))
        .collect();
    nodes.sort_by_key(|(freq, _)| *freq);
    while nodes.len() > 1 {
        let (freq1, syms1) = nodes.remove(0);
        let (freq2, syms2) = nodes.remove(0);
        let combined = freq1 + freq2;
        let mut syms = syms1;
        syms.extend(syms2);
        for symbol in &syms {
            lengths[*symbol] += 1;
        }
        let position = nodes
            .iter()
            .position(|(freq, _)| *freq > combined)
            .unwrap_or(nodes.len());
        nodes.insert(position, (combined, syms));
    }
    limit_code_lengths(&mut lengths, max_len);
    lengths
}

/// 限制码长并保持 Kraft 不等式。
fn limit_code_lengths(lengths: &mut [u8], max_len: usize) {
    let max_len = max_len as u8;
    if !lengths.iter().any(|value| *value > max_len) {
        return;
    }
    let mut items: Vec<(usize, u8)> = lengths
        .iter()
        .enumerate()
        .filter(|(_, value)| **value > 0)
        .map(|(index, value)| (index, *value))
        .collect();
    items.sort_by_key(|(_, value)| std::cmp::Reverse(*value));
    for (symbol, value) in &mut items {
        if *value > max_len {
            *value = max_len;
            lengths[*symbol] = max_len;
        }
    }
    loop {
        let scale = 1u64 << max_len;
        let kraft: u64 = lengths
            .iter()
            .filter(|value| **value > 0)
            .map(|value| scale >> *value)
            .sum();
        if kraft <= scale {
            break;
        }
        let mut increased = false;
        for value in lengths.iter_mut() {
            if *value > 0 && *value < max_len {
                *value += 1;
                increased = true;
                break;
            }
        }
        if !increased {
            break;
        }
    }
}

/// 从码长生成规范 Huffman 码。
fn build_codes(lengths: &[u8]) -> Vec<u32> {
    let mut codes = vec![0u32; lengths.len()];
    if lengths.is_empty() {
        return codes;
    }
    let max_len = *lengths.iter().max().unwrap_or(&0) as usize;
    let mut counts = vec![0u32; max_len + 1];
    for value in lengths {
        if *value > 0 {
            counts[*value as usize] += 1;
        }
    }
    let mut next = vec![0u32; max_len + 1];
    let mut code = 0u32;
    for bits in 1..=max_len {
        code = (code + counts[bits - 1]) << 1;
        next[bits] = code;
    }
    for (symbol, value) in lengths.iter().enumerate() {
        if *value > 0 {
            codes[symbol] = next[*value as usize];
            next[*value as usize] += 1;
        }
    }
    codes
}
