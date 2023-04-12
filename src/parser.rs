use std::collections::HashMap;

use makai::utils::bytes::ByteStorage;
use makai_waveform_db::{bitvector::BitVector, Waveform};

use crate::errors::*;
use crate::lexer::position::LexerPosition;
use crate::tokenizer::token::*;

// Returns the timescale resolution x, where x is 10^(-x)
pub fn convert_timescale(timescale: TokenTimescale, offset: TokenTimescaleOffset) -> i32 {
    let base = match timescale {
        TokenTimescale::Femtoseconds => 15,
        TokenTimescale::Picoseconds => 12,
        TokenTimescale::Nanoseconds => 9,
        TokenTimescale::Microseconds => 6,
        TokenTimescale::Milliseconds => 3,
        TokenTimescale::Seconds => 0,
    };
    let offset = match offset {
        TokenTimescaleOffset::One => 0,
        TokenTimescaleOffset::Ten => -1,
        TokenTimescaleOffset::Hundred => -2,
    };
    base + offset
}

pub type VcdVariableNetType = TokenVariableNetType;
pub type VcdScopeType = TokenScopeType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VcdVariableWidth {
    Vector { width: usize },
    Real,
}

impl VcdVariableWidth {
    pub fn get_width(&self) -> usize {
        match self {
            Self::Vector { width } => *width,
            Self::Real => 64,
        }
    }
}

impl std::fmt::Display for VcdVariableWidth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vector { width } => match width {
                0 => write!(f, "[empty]"),
                1 => write!(f, ""),
                _ => write!(f, "[{}]", width),
            },
            Self::Real => write!(f, "[real]"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcdVariableDescription {
    Unspecified,
    Vector { width: usize },
    VectorSelect { msb: usize, lsb: usize },
}

impl VcdVariableDescription {
    pub fn new(description: TokenVariableDescription) -> Self {
        match description {
            TokenVariableDescription::Unspecified { id: _ } => Self::Unspecified,
            TokenVariableDescription::Vector { id: _, width } => Self::Vector { width },
            TokenVariableDescription::VectorSelect { id: _, msb, lsb } => {
                Self::VectorSelect { msb, lsb }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VcdVariable {
    name: String,
    description: VcdVariableDescription,
    width: VcdVariableWidth,
    net_type: VcdVariableNetType,
    idcode: usize,
}

impl VcdVariable {
    pub fn new(
        token_width: usize,
        description: TokenVariableDescription,
        net_type: TokenVariableNetType,
        token_idcode: TokenIdCode,
        pos: &LexerPosition,
        bs: &ByteStorage,
    ) -> ParserResult<Self> {
        let (name_id, width) = match net_type {
            VcdVariableNetType::Real | VcdVariableNetType::Realtime => match description {
                TokenVariableDescription::Unspecified { id } => (id, VcdVariableWidth::Real),
                _ => return Err(ParserError::MismatchedWidth(*pos)),
            },
            _ => match description {
                TokenVariableDescription::Unspecified { id } => {
                    (id, VcdVariableWidth::Vector { width: token_width })
                }
                TokenVariableDescription::Vector { id, width } => {
                    if width != token_width {
                        return Err(ParserError::MismatchedWidth(*pos));
                    }
                    (id, VcdVariableWidth::Vector { width: token_width })
                }
                TokenVariableDescription::VectorSelect { id, msb, lsb } => {
                    let width = msb - lsb + 1;
                    if width != token_width {
                        return Err(ParserError::MismatchedWidth(*pos));
                    }
                    (id, VcdVariableWidth::Vector { width })
                }
            },
        };
        Ok(Self {
            name: String::from_utf8_lossy(&bs.get_bytes(name_id)).to_string(),
            description: VcdVariableDescription::new(description),
            width,
            net_type,
            idcode: token_idcode.get_id(),
        })
    }

    pub fn get_name(&self) -> &String {
        &self.name
    }

    pub fn get_width(&self) -> &VcdVariableWidth {
        &self.width
    }

    pub fn get_bit_width(&self) -> usize {
        self.width.get_width()
    }

    pub fn get_idcode(&self) -> usize {
        self.idcode
    }
}

impl std::fmt::Display for VcdVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.name, self.width)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VcdScope {
    name: String,
    scope_type: VcdScopeType,
    scopes: Vec<VcdScope>,
    variables: Vec<VcdVariable>,
}

impl VcdScope {
    pub fn new(name_id: usize, scope_type: TokenScopeType, bs: &ByteStorage) -> Self {
        Self {
            name: String::from_utf8_lossy(&bs.get_bytes(name_id)).to_string(),
            scope_type,
            scopes: Vec::new(),
            variables: Vec::new(),
        }
    }

    pub fn get_name(&self) -> &String {
        &self.name
    }

    pub fn get_type(&self) -> &VcdScopeType {
        &self.scope_type
    }

    pub fn get_scopes(&self) -> &Vec<VcdScope> {
        &self.scopes
    }

    pub fn get_variables(&self) -> &Vec<VcdVariable> {
        &self.variables
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum VcdEntry {
    Timestamp(u64),
    Vector(BitVector, usize),
    Real(f64, usize),
}

impl Default for VcdEntry {
    fn default() -> Self {
        Self::Timestamp(0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VcdHeader {
    version: Option<String>,
    date: Option<String>,
    timescale: Option<i32>,
    idcodes: HashMap<usize, VcdVariableWidth>, // id, width
    scopes: Vec<VcdScope>,
}

fn get_scope_recursive<'a>(scope: &'a VcdScope, path: &str) -> Option<&'a VcdScope> {
    let sections: Vec<&str> = path.split('.').collect();
    for scope in &scope.scopes {
        if sections.is_empty() {
            return None;
        } else if scope.get_name() == sections[0] {
            if sections.len() > 1 {
                return get_scope_recursive(scope, &sections[1..].join("."));
            } else {
                return Some(scope);
            }
        }
    }
    None
}

fn get_variable_recursive<'a>(scope: &'a VcdScope, path: &str) -> Option<&'a VcdVariable> {
    let sections: Vec<&str> = path.split('.').collect();
    match sections.len() {
        0 => {}
        1 => {
            for variable in &scope.variables {
                if variable.get_name() == sections[0] {
                    return Some(variable);
                }
            }
        }
        _ => {
            for scope in &scope.scopes {
                if scope.get_name() == sections[0] {
                    return get_variable_recursive(scope, &sections[1..].join("."));
                }
            }
        }
    }
    None
}

impl VcdHeader {
    pub fn new() -> Self {
        Self {
            version: None,
            date: None,
            timescale: None,
            idcodes: HashMap::new(),
            scopes: Vec::new(),
        }
    }

    /// Initializes the specified waveform object with the variables defined by this header
    pub fn initialize_waveform(&self, waveform: &mut Waveform) {
        for (idcode, width) in self.get_idcodes_map().iter() {
            match width {
                VcdVariableWidth::Vector { width } => {
                    waveform.initialize_vector(*idcode, *width);
                }
                VcdVariableWidth::Real => {
                    waveform.initialize_real(*idcode);
                }
            }
        }
    }

    /// Returns a reference to all scopes in this header, and each of those scopes may or may not
    /// contain variables or other scopes
    pub fn get_scopes(&self) -> &Vec<VcdScope> {
        &self.scopes
    }

    /// Returns a scope that matches the given path, where nested scopes can be specified with dots
    /// i.e. `get_scope("foo.bar")` will return the scope of name "bar" contained in the scope of
    /// name "foo"
    pub fn get_scope(&self, path: &str) -> Option<&VcdScope> {
        let sections: Vec<&str> = path.split('.').collect();
        for scope in &self.scopes {
            if sections.is_empty() {
                return None;
            } else if scope.get_name() == sections[0] {
                if sections.len() > 1 {
                    return get_scope_recursive(scope, &sections[1..].join("."));
                } else {
                    return Some(scope);
                }
            }
        }
        None
    }

    /// Returns a variable that matches the given path, where the nesting scopes can be specified
    /// with dots i.e. `get_variable("foo.test")` will return the variable of name "test" contained
    /// in the scope of name "foo"
    pub fn get_variable(&self, path: &str) -> Option<&VcdVariable> {
        let sections: Vec<&str> = path.split('.').collect();
        for scope in &self.scopes {
            if sections.len() < 2 {
                return None;
            } else if scope.get_name() == sections[0] {
                return get_variable_recursive(scope, &sections[1..].join("."));
            }
        }
        None
    }

    pub fn get_idcodes_map(&self) -> &HashMap<usize, VcdVariableWidth> {
        &self.idcodes
    }

    pub fn get_version(&self) -> &Option<String> {
        &self.version
    }

    pub fn get_date(&self) -> &Option<String> {
        &self.date
    }

    pub fn get_timescale(&self) -> &Option<i32> {
        &self.timescale
    }
}

impl Default for VcdHeader {
    fn default() -> Self {
        Self::new()
    }
}

pub struct VcdReader {
    bs: ByteStorage,
    header: VcdHeader,
    scope_depth: usize,
}

impl VcdReader {
    pub fn new() -> Self {
        Self {
            bs: ByteStorage::new(),
            header: VcdHeader::new(),
            scope_depth: 0,
        }
    }

    pub fn get_byte_storage(&self) -> &ByteStorage {
        &self.bs
    }

    pub fn get_byte_storage_mut(&mut self) -> &mut ByteStorage {
        &mut self.bs
    }

    pub fn get_header(&self) -> &VcdHeader {
        &self.header
    }

    pub fn into_header(self) -> VcdHeader {
        self.header
    }

    pub fn parse_header<F>(&mut self, token_generator: &mut F) -> ParserResult<()>
    where
        F: FnMut(&mut ByteStorage) -> TokenizerResult<Option<Token>>,
    {
        loop {
            let token = match token_generator(&mut self.bs) {
                Ok(Some(token)) => token,
                Ok(None) => return Err(ParserError::UnexpectedTermination),
                Err(err) => return Err(ParserError::Tokenizer(err)),
            };
            match token {
                Token::Comment(_, _) => {}
                Token::Date(id, _) => {
                    self.header.date =
                        Some(String::from_utf8_lossy(&self.bs.get_bytes(id)).to_string());
                }
                Token::Version(id, _) => {
                    self.header.version =
                        Some(String::from_utf8_lossy(&self.bs.get_bytes(id)).to_string());
                }
                Token::Timescale {
                    timescale,
                    offset,
                    pos: _,
                } => {
                    self.header.timescale = Some(convert_timescale(timescale, offset));
                }
                Token::Scope {
                    scope_type,
                    scope_id,
                    pos: _,
                } => {
                    let mut scopes = &mut self.header.scopes;
                    for _ in 0..self.scope_depth {
                        scopes = &mut scopes.last_mut().unwrap().scopes;
                    }
                    scopes.push(VcdScope::new(scope_id, scope_type, &self.bs));
                    self.scope_depth += 1;
                }
                Token::Var {
                    net_type,
                    width,
                    token_idcode,
                    variable_description,
                    pos,
                } => {
                    if self.scope_depth == 0 {
                        return Err(ParserError::UnexpectedVariable(pos));
                    }
                    let variable = VcdVariable::new(
                        width,
                        variable_description,
                        net_type,
                        token_idcode.clone(),
                        &pos,
                        &self.bs,
                    )?;
                    if let Some(old_width) = self
                        .header
                        .idcodes
                        .insert(token_idcode.get_id(), variable.width.clone())
                    {
                        if old_width != variable.width.clone() {
                            return Err(ParserError::UnmatchedIdcode(pos));
                        }
                    }
                    let mut scopes = &mut self.header.scopes;
                    for _ in 0..self.scope_depth - 1 {
                        scopes = &mut scopes.last_mut().unwrap().scopes;
                    }
                    scopes.last_mut().unwrap().variables.push(variable);
                }
                Token::UpScope(pos) => {
                    if self.scope_depth == 0 {
                        return Err(ParserError::UnexpectedUpscope(pos));
                    }
                    self.scope_depth -= 1;
                }
                Token::EndDefinitions(pos) => {
                    if self.scope_depth != 0 {
                        return Err(ParserError::UnexpectedEndDefinitions(pos));
                    }
                    return Ok(());
                }
                t => return Err(ParserError::UnexpectedToken(t)),
            }
        }
    }

    pub fn parse_waveform<F>(&mut self, token_generator: &mut F) -> ParserResult<Option<VcdEntry>>
    where
        F: FnMut(&mut ByteStorage) -> TokenizerResult<Option<Token>>,
    {
        let entry = loop {
            let token = match token_generator(&mut self.bs) {
                Ok(Some(token)) => token,
                Ok(None) => return Ok(None),
                Err(err) => return Err(ParserError::Tokenizer(err)),
            };
            match token {
                Token::Timestamp(timestamp, _) => break VcdEntry::Timestamp(timestamp),
                Token::VectorValue(bv, idcode, _) => break VcdEntry::Vector(bv, idcode.get_id()),
                Token::RealValue(value, idcode, _) => break VcdEntry::Real(value, idcode.get_id()),
                // Ignore these tokens
                Token::Comment(_, _) => {}
                Token::DumpAll(_) => {}
                Token::DumpOff(_) => {}
                Token::DumpOn(_) => {}
                Token::DumpVars(_) => {}
                Token::End(_) => {}
                t => return Err(ParserError::UnexpectedToken(t)),
            }
        };

        Ok(Some(entry))
    }
}

impl Default for VcdReader {
    fn default() -> Self {
        Self::new()
    }
}
