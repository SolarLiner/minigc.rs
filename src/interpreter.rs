use std::collections::HashMap;
use std::io::Write;
use std::ops::{Add, Mul, Sub};

use generational_arena::{Arena, Index};
use tracing::{error, instrument, trace};

use crate::traits::{DisplayArena, Tree};
use crate::vm::{Object, VM};
use std::fmt;
use thiserror::Error;

#[derive(Clone, Debug)]
pub enum Value {
    Int(i32),
    Float(f32),
    Struct(Vec<Index>),
}

impl DisplayArena for Value {
    type Type = Self;

    fn display_arena(
        &self,
        f: &mut dyn Write,
        arena: &Arena<Object<Self::Type>>,
    ) -> std::io::Result<()> {
        match self {
            Self::Int(i) => write!(f, "{}i", i),
            Self::Float(fl) => write!(f, "{}f", fl),
            Self::Struct(v) => {
                write!(f, "Struct(")?;
                for (i, obj) in v.iter().copied().map(|i| &arena[i]).enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    obj.value.display_arena(f, arena)?;
                }
                write!(f, ")")
            }
        }
    }
}

impl Tree<Index> for Value {
    fn children(&self) -> Vec<Index> {
        match self {
            Value::Struct(v) => v.clone(),
            Value::Int(_) | Value::Float(_) => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum Instruction {
    ConstInt(i32),
    ConstFloat(f32),
    PushStruct(usize),
    GetStruct(usize),
    GetLocal(usize),
    Label(String),
    IAdd,
    FAdd,
    ISub,
    FSub,
    IMul,
    FMul,
    Call(String, usize),
    Jump(String),
    JmpCmp(String),
    CEq,
    CNe,
    CLt,
    CLe,
    CGt,
    CGe,
    Return,
}

enum InstructionResult {
    Next,
    Jump(Index),
    Stop,
}

#[derive(Debug, Error)]
pub enum InterpreterError {
    #[error("Value type mismatch, unexpected {0}")]
    ValueMismatch(String),
    #[error("Invalid instruction pointer state")]
    InvalidInstructionPointer,
    #[error("Interpreter stack underflow")]
    StackUnderflow,
    #[error("Unresolved label {0}")]
    UnresolvedLabel(String),
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
}

pub struct Frame {
    instr_ptr: Index,
    locals_stack: Vec<Vec<Index>>,
}

impl Frame {
    #[instrument(skip(self))]
    pub(crate) fn move_to(&mut self, id: Index) {
        self.instr_ptr = id;
    }
}

pub struct Interpreter {
    vm: VM<Value>,
    instructions: Arena<Instruction>,
    labels: HashMap<String, Index>,
    frame: Option<Frame>,
}

impl Interpreter {
    pub(crate) fn display(&self, id: Index) -> std::io::Result<impl fmt::Display> {
        self.vm.display(id)
    }
}

macro_rules! binop {
    ($self:expr, $v:path, $e:expr) => {
        match $self.get_binop()? {
            (Object { value: $v(a), .. }, Object { value: $v(b), .. }) => {
                let a = a.clone();
                let b = b.clone();
                $self.vm.push_value($v($e(a, &b)));
            }
            (Object { index: ia, .. }, Object { index: ib, .. }) => {
                let ia = *ia;
                let ib = *ib;
                return Err(InterpreterError::ValueMismatch(format!(
                    "{}, {}",
                    $self.vm.display(ia)?,
                    $self.vm.display(ib)?
                )));
            }
        }
    };
}

macro_rules! cmp {
    ($self:expr, $v:expr) => {
        match $self.get_binop()? {
            (
                Object {
                    value: Value::Int(a),
                    ..
                },
                Object {
                    value: Value::Int(b),
                    ..
                },
            ) => {
                let res = i32::from($v(&a, &b));
                $self.vm.push_value(Value::Int(res));
            },
            (
                Object {
                    value: Value::Float(a),
                    ..
                },
                Object {
                    value: Value::Float(b),
                    ..
                },
            ) => {
                let res = i32::from($v(&a, &b));
                $self.vm.push_value(Value::Int(res));
            }
            (Object { index: ia, .. }, Object { index: ib, .. }) => {
                let ia = *ia;
                let ib = *ib;
                return Err(InterpreterError::ValueMismatch(format!(
                    "{}, {}",
                    $self.vm.display(ia)?,
                    $self.vm.display(ib)?
                )));
            }
        }
    };
}

impl Interpreter {
    pub fn new(input_instructions: Vec<Instruction>) -> Option<Self> {
        let instructions = Arena::with_capacity(input_instructions.len());
        let mut this = Self {
            vm: VM::new(10),
            instructions,
            labels: HashMap::new(),
            frame: None,
        };

        let mut next_label = None;
        let mut first_instr = None;
        for instruction in input_instructions {
            match instruction {
                Instruction::Label(s) => next_label = Some(s),
                instr => {
                    let idx = this.instructions.insert(instr);
                    first_instr = first_instr.or(Some(idx));
                    if let Some(lbl) = next_label.take() {
                        this.labels.insert(lbl, idx);
                    }
                }
            }
        }
        this.frame = first_instr.map(|instr_ptr| Frame {
            locals_stack: vec![],
            instr_ptr,
        });
        return Some(this);
    }

    #[instrument(skip(self))]
    pub fn run(&mut self) -> Result<Index, InterpreterError> {
        while let Some(instr) = self
            .frame
            .as_ref()
            .and_then(|f| {
                trace!(instr_ptr = ?f.instr_ptr);
                self.instructions.get(f.instr_ptr)
            })
            .cloned()
        {
            trace!(?instr);
            match self.execute(instr)? {
                InstructionResult::Next => {
                    let frame = self.frame.as_mut().unwrap();
                    if let Some((_, idx)) = self.instructions.get_unknown_gen(frame.instr_ptr.into_raw_parts().0+1) {
                        frame.move_to(idx);
                    } else {
                        break;
                    }
                }
                InstructionResult::Jump(j) => self.frame.as_mut().unwrap().move_to(j),
                InstructionResult::Stop => break,
            }
        }
        self.vm.top().ok_or(InterpreterError::StackUnderflow)
    }

    #[instrument(skip(self))]
    fn execute(&mut self, instr: Instruction) -> Result<InstructionResult, InterpreterError> {
        trace!(?instr);
        match instr {
            Instruction::ConstInt(i) => self.vm.push_value(Value::Int(i)),
            Instruction::ConstFloat(f) => self.vm.push_value(Value::Float(f)),
            Instruction::PushStruct(s) => {
                let vals = (0..s)
                    .map(|_| self.vm.pop())
                    .rev()
                    .collect::<Option<Vec<_>>>().ok_or(InterpreterError::StackUnderflow)?;
                self.vm.push_value(Value::Struct(vals));
            }
            Instruction::GetLocal(i) => {
                let s = self.frame.as_ref().unwrap().locals_stack.last().ok_or(InterpreterError::StackUnderflow)?;
                self.vm.push(s[i]);
            }
            Instruction::GetStruct(i) => {
                let s = self.vm.pop().ok_or(InterpreterError::StackUnderflow)?;
                match self.vm.get(s) {
                    Object {
                        value: Value::Struct(s),
                        ..
                    } => self.vm.push(s[i]),
                    _ => {
                        return Err(InterpreterError::ValueMismatch(self.vm.display(s)?.to_string()));
                    }
                }
            }
            Instruction::Label(_) => {}
            Instruction::IAdd => binop!(self, Value::Int, i32::add),
            Instruction::FAdd => binop!(self, Value::Float, f32::add),
            Instruction::ISub => binop!(self, Value::Int, i32::sub),
            Instruction::FSub => binop!(self, Value::Float, f32::sub),
            Instruction::IMul => binop!(self, Value::Int, i32::mul),
            Instruction::FMul => binop!(self, Value::Float, f32::mul),
            Instruction::Call(s, n) => {
                let next_ptr = self.labels.get(&s).copied().ok_or(InterpreterError::UnresolvedLabel(s))?;
                let locals = (0..n)
                    .map(|_| self.vm.pop())
                    .rev()
                    .collect::<Option<Vec<_>>>().ok_or(InterpreterError::StackUnderflow)?;
                self.frame.as_mut().unwrap().locals_stack.push(locals);
                return Ok(InstructionResult::Jump(next_ptr));
            }
            Instruction::Jump(s) => {
                let next_ptr = self.labels.get(&s).copied().ok_or(InterpreterError::UnresolvedLabel(s))?;
                return Ok(InstructionResult::Jump(next_ptr));
            }
            Instruction::JmpCmp(s) => {
                let instr_ptr = self.labels.get(&s).copied().ok_or(InterpreterError::UnresolvedLabel(s))?;
                let val = self.vm.pop().ok_or(InterpreterError::StackUnderflow)?;
                let cmp = self.vm.get(val);
                return match &cmp.value {
                    Value::Int(1) => Ok(InstructionResult::Jump(instr_ptr)),
                    Value::Int(0) => Ok(InstructionResult::Next),
                    _ => {
                        error!("Value mismatch {}", self.vm.display(cmp.index)?);
                        Err(InterpreterError::ValueMismatch(self.vm.display(cmp.index)?.to_string()))
                    }
                };
            }
            Instruction::CEq => cmp!(self, PartialEq::eq),
            Instruction::CNe => cmp!(self, PartialEq::ne),
            Instruction::CLt => cmp!(self, PartialOrd::lt),
            Instruction::CLe => cmp!(self, PartialOrd::le),
            Instruction::CGt => cmp!(self, PartialOrd::gt),
            Instruction::CGe => cmp!(self, PartialOrd::ge),
            Instruction::Return => return Ok(InstructionResult::Stop),
        }
        return Ok(InstructionResult::Next);
    }

    fn get_binop(&mut self) -> Result<(&Object<Value>, &Object<Value>), InterpreterError> {
        let b = self.vm.pop().ok_or(InterpreterError::StackUnderflow)?;
        let a = self.vm.pop().ok_or(InterpreterError::StackUnderflow)?;
        return Ok((self.vm.get(a), self.vm.get(b)));
    }
}
