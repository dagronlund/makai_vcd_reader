#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LexerPosition {
    index: usize,
    line: usize,
    column: usize,
    length: usize,
}

impl LexerPosition {
    pub fn new(index: usize, line: usize, column: usize, length: usize) -> Self {
        Self {
            index,
            line,
            column,
            length,
        }
    }

    pub fn get_index(&self) -> usize {
        self.index
    }

    pub fn get_line(&self) -> usize {
        self.line
    }

    pub fn get_column(&self) -> usize {
        self.column
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}
