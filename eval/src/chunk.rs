use crate::opcode::{CodeIdx, ConstantIdx, OpCode};
use crate::value::Value;

#[derive(Debug, Default)]
pub struct Chunk {
    pub code: Vec<OpCode>,
    pub constants: Vec<Value>,
}

impl Chunk {
    pub fn push_op(&mut self, data: OpCode) -> CodeIdx {
        let idx = self.code.len();
        self.code.push(data);
        CodeIdx(idx)
    }

    pub fn push_constant(&mut self, data: Value) -> ConstantIdx {
        let idx = self.constants.len();
        self.constants.push(data);
        ConstantIdx(idx)
    }

    pub fn constant(&self, idx: ConstantIdx) -> &Value {
        &self.constants[idx.0]
    }
}
