use std::io;

use indiscriminant::indiscriminant;
use makai::utils::bytes::ByteStorage;
use makai_waveform_db::bitvector::BitVector;

use crate::lexer::position::*;

fn bitvector_write_to(bv: &BitVector, writer: &mut dyn io::Write) -> io::Result<usize> {
    if bv.get_bit_width() == 1 {
        writer.write(bv.get_bit(0).to_str().as_bytes())
    } else {
        let mut size = 0;
        size += writer.write(b"b")?;
        for i in 0..bv.get_bit_width() {
            size += writer.write(bv.get_bit(i).to_str().as_bytes())?;
        }
        size += writer.write(b" ")?;
        Ok(size)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenIdCode {
    id: usize,
}

impl TokenIdCode {
    pub fn new(id: usize) -> Self {
        Self { id }
    }

    pub fn write_to(&self, bs: &ByteStorage, writer: &mut dyn io::Write) -> io::Result<usize> {
        let mask = 1 << (usize::BITS - 1);
        if self.id & mask == 0 {
            let mut size = 0;
            let mut id = self.id;
            for _ in 0..8 {
                let b = (id & 0xff) as u8;
                if b == 0 {
                    break;
                }
                size += writer.write(&[b])?;
                id >>= 8;
            }
            Ok(size)
        } else {
            let mut size = 0;
            let bytes = bs.get_bytes(self.id & !mask);
            size += writer.write(&bytes)?;
            Ok(size)
        }
    }

    pub fn get_id(&self) -> usize {
        self.id
    }
}

#[indiscriminant()]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenTimescaleOffset {
    One = b"1",
    Ten = b"10",
    Hundred = b"100",
}

#[indiscriminant()]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenTimescale {
    Femtoseconds = b"fs",
    Picoseconds = b"ps",
    Nanoseconds = b"ns",
    Microseconds = b"us",
    Milliseconds = b"ms",
    Seconds = b"s",
}

#[indiscriminant()]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenScopeType {
    Module = b"module",
    Task = b"task",
    Function = b"function",
    Begin = b"begin",
    Fork = b"fork",
    Struct = b"struct",
    Union = b"union",
    Interface = b"interface",
}

#[indiscriminant()]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenVariableNetType {
    Event = b"event",
    Integer = b"integer",
    Parameter = b"parameter",
    Real = b"real",
    Realtime = b"realtime",
    Reg = b"reg",
    Supply0 = b"supply0",
    Supply1 = b"supply1",
    Time = b"time",
    Tri = b"tri",
    Triand = b"triand",
    Trior = b"trior",
    Trireg = b"trireg",
    Tri0 = b"tri0",
    Tri1 = b"tri1",
    Wand = b"wand",
    Wire = b"wire",
    Wor = b"wor",
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenVariableDescription {
    Unspecified { id: usize },
    Vector { id: usize, width: usize },
    VectorSelect { id: usize, msb: usize, lsb: usize },
}

impl TokenVariableDescription {
    pub fn get_id(&self) -> usize {
        match self {
            Self::Unspecified { id } => *id,
            Self::Vector { id, width: _ } => *id,
            Self::VectorSelect { id, msb: _, lsb: _ } => *id,
        }
    }

    pub fn get_width(&self) -> usize {
        match self {
            Self::Unspecified { id: _ } => 0,
            Self::Vector { id: _, width } => *width,
            Self::VectorSelect { id: _, msb, lsb } => *msb - *lsb + 1,
        }
    }

    pub fn write_to(&self, bs: &ByteStorage, writer: &mut dyn io::Write) -> io::Result<usize> {
        match self {
            Self::Unspecified { id } => writer.write(&bs.get_bytes(*id)),
            Self::Vector { id, width } => {
                let mut size = 0;
                size += writer.write(&bs.get_bytes(*id))?;
                size += writer.write(format!("[{}]", width).as_bytes())?;
                Ok(size)
            }
            Self::VectorSelect { id, msb, lsb } => {
                let mut size = 0;
                size += writer.write(&bs.get_bytes(*id))?;
                size += writer.write(format!("[{}:{}]", msb, lsb).as_bytes())?;
                Ok(size)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Unformatted blocks
    Comment(usize, LexerPosition),
    Date(usize, LexerPosition),
    Version(usize, LexerPosition),
    // Formatted blocks
    Scope {
        scope_type: TokenScopeType,
        scope_id: usize,
        pos: LexerPosition,
    },
    Timescale {
        timescale: TokenTimescale,
        offset: TokenTimescaleOffset,
        pos: LexerPosition,
    },
    Var {
        net_type: TokenVariableNetType,
        width: usize,
        token_idcode: TokenIdCode,
        variable_description: TokenVariableDescription,
        pos: LexerPosition,
    },
    // Empty blocks
    UpScope(LexerPosition),
    EndDefinitions(LexerPosition),
    // Waveform signals
    DumpAll(LexerPosition),
    DumpOff(LexerPosition),
    DumpOn(LexerPosition),
    DumpVars(LexerPosition),
    End(LexerPosition),
    Timestamp(u64, LexerPosition),
    VectorValue(BitVector, TokenIdCode, LexerPosition),
    RealValue(f64, TokenIdCode, LexerPosition),
}

impl Token {
    fn write_to_block(
        &self,
        bs: &ByteStorage,
        writer: &mut dyn io::Write,
        id: &usize,
        header: &'static [u8],
    ) -> io::Result<usize> {
        let mut size = 0;
        size += writer.write(b"$")?;
        size += writer.write(header)?;
        size += writer.write(&bs.get_bytes(*id))?;
        size += writer.write(b"$end\n")?;
        Ok(size)
    }

    pub fn write_to(&self, bs: &ByteStorage, writer: &mut dyn io::Write) -> io::Result<usize> {
        let bytes = match self {
            Self::Comment(id, _) => self.write_to_block(bs, writer, id, b"comment")?,
            Self::Date(id, _) => self.write_to_block(bs, writer, id, b"date")?,
            Self::Version(id, _) => self.write_to_block(bs, writer, id, b"version")?,
            Self::Scope {
                scope_type,
                scope_id,
                pos: _,
            } => {
                let mut size = 0;
                size += writer.write(b"$scope ")?;
                size += writer.write(scope_type.to_byte_str())?;
                size += writer.write(b" ")?;
                size += writer.write(&bs.get_bytes(*scope_id))?;
                size += writer.write(b" $end\n")?;
                size
            }
            Self::Timescale {
                timescale,
                offset,
                pos: _,
            } => {
                let mut size = 0;
                size += writer.write(b"$timescale ")?;
                size += writer.write(offset.to_byte_str())?;
                size += writer.write(b" ")?;
                size += writer.write(timescale.to_byte_str())?;
                size += writer.write(b" $end\n")?;
                size
            }
            Self::Var {
                net_type,
                width,
                token_idcode,
                variable_description,
                pos: _,
            } => {
                let mut size = 0;
                size += writer.write(b"$var ")?;
                size += writer.write(net_type.to_byte_str())?;
                size += writer.write(b" ")?;
                size += writer.write(format!("{}", width).as_bytes())?;
                size += writer.write(b" ")?;
                size += token_idcode.write_to(bs, writer)?;
                size += writer.write(b" ")?;
                size += variable_description.write_to(bs, writer)?;
                size += writer.write(b" $end\n")?;
                size
            }
            Self::UpScope(_) => writer.write(b"$upscope $end\n")?,
            Self::EndDefinitions(_) => writer.write(b"$enddefinitions $end\n")?,
            Self::DumpAll(_) => writer.write(b"$dumpall\n")?,
            Self::DumpOff(_) => writer.write(b"$dumpoff\n")?,
            Self::DumpOn(_) => writer.write(b"$dumpon\n")?,
            Self::DumpVars(_) => writer.write(b"$dumpvars\n")?,
            Self::End(_) => writer.write(b"$end\n")?,
            Self::Timestamp(t, _) => writer.write(format!("#{}\n", t).as_bytes())?,
            Self::VectorValue(bv, idcode, _) => {
                let mut size = 0;
                size += bitvector_write_to(bv, writer)?;
                size += idcode.write_to(bs, writer)?;
                size += writer.write(b"\n")?;
                size
            }
            Self::RealValue(r, idcode, _) => {
                let mut size = 0;
                size += writer.write(format!("r{:.16} ", r).as_bytes())?;
                size += idcode.write_to(bs, writer)?;
                size += writer.write(b"\n")?;
                size
            }
        };
        Ok(bytes)
    }

    pub fn get_position(&self) -> LexerPosition {
        match self {
            Self::Comment(_, pos)
            | Self::Date(_, pos)
            | Self::Version(_, pos)
            | Self::Scope {
                scope_type: _,
                scope_id: _,
                pos,
            }
            | Self::Timescale {
                timescale: _,
                offset: _,
                pos,
            }
            | Self::Var {
                net_type: _,
                width: _,
                token_idcode: _,
                variable_description: _,
                pos,
            }
            | Self::UpScope(pos)
            | Self::EndDefinitions(pos)
            | Self::DumpAll(pos)
            | Self::DumpOff(pos)
            | Self::DumpOn(pos)
            | Self::DumpVars(pos)
            | Self::End(pos)
            | Self::Timestamp(_, pos)
            | Self::VectorValue(_, _, pos)
            | Self::RealValue(_, _, pos) => *pos,
        }
    }

    pub fn get_index(&self) -> usize {
        self.get_position().get_index()
    }

    pub fn len(&self) -> usize {
        self.get_position().len()
    }

    pub fn is_empty(&self) -> bool {
        self.get_position().len() == 0
    }
}
