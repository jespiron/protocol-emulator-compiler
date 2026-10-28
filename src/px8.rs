//! Assembler for the PX8 controller (`src/px8_controller.v`).
//!
//! Every instruction is one byte: a 4-bit opcode and a 4-bit operand. Wider
//! operands are built from `PFIX` prefix bytes. Jump and data-pointer targets
//! always use three nibbles so forward references have a fixed size.
//!
//! Consecutive configuration writes are collected into one block load
//! (`SETP table`, `SETC n`, `CFG`) whose `{address, low, high}` entries are
//! placed after the code. A lone write stays `LDC value`, `WCF address`.
//! A block load leaves `C = 0` and moves `P`.
use crate::hardware::SRAM_BYTES;

const PFIX: u8 = 0x0;
const LDC: u8 = 0x1;
const SETP: u8 = 0x2;
const J: u8 = 0x4;
const SETC: u8 = 0x7;
const WCF: u8 = 0x8;
const OPR: u8 = 0xE;
const OPR_CFG: u16 = 9;

/// A label: an instruction address resolved at assembly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Label(usize);

enum Target {
    Label(Label),
    Data(usize),
}

#[derive(Default)]
pub(crate) struct Assembler {
    code: Vec<u8>,
    data: Vec<u8>,
    labels: Vec<Option<usize>>,
    fixups: Vec<(usize, u8, Target)>,
    pending: Vec<(u8, u16)>,
}

impl Assembler {
    /// Queue a configuration write; adjacent writes share one block load.
    pub(crate) fn config(&mut self, address: u8, value: u16) {
        self.pending.push((address, value));
    }

    fn flush(&mut self) {
        let writes = std::mem::take(&mut self.pending);
        if writes.len() == 1 {
            for (address, value) in writes {
                self.emit(LDC, value);
                self.emit(WCF, u16::from(address));
            }
        } else if !writes.is_empty() {
            self.fixed(SETP, Target::Data(self.data.len()));
            for (address, value) in &writes {
                self.data.push(*address);
                self.data.extend(value.to_le_bytes());
            }
            self.emit(SETC, writes.len() as u16);
            self.emit(OPR, OPR_CFG);
        }
    }

    fn emit(&mut self, opcode: u8, operand: u16) {
        self.flush();
        let digits = (0..4)
            .rev()
            .map(|i| ((operand >> (4 * i)) & 0xF) as u8)
            .skip_while(|&d| d == 0)
            .collect::<Vec<_>>();
        let (last, prefixes) = digits.split_last().unwrap_or((&0, &[]));
        self.code.extend(prefixes.iter().map(|d| PFIX << 4 | d));
        self.code.push(opcode << 4 | last);
    }

    /// Three-nibble instruction patched once its target address is known.
    fn fixed(&mut self, opcode: u8, target: Target) {
        self.fixups.push((self.code.len(), opcode, target));
        self.code.extend([0, 0, 0]);
    }

    pub(crate) fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    pub(crate) fn place(&mut self, label: Label) {
        self.flush();
        self.labels[label.0] = Some(self.code.len());
    }

    pub(crate) fn jump(&mut self, label: Label) {
        self.flush();
        self.fixed(J, Target::Label(label));
    }

    /// Spin forever; the fabric keeps running on its own.
    pub(crate) fn idle_forever(&mut self) {
        let idle = self.label();
        self.place(idle);
        self.jump(idle);
    }

    /// Lay out code then data, resolving every target.
    pub(crate) fn assemble(mut self) -> Result<Vec<u8>, String> {
        self.flush();
        let data_start = self.code.len();
        let mut image = std::mem::take(&mut self.code);
        for &(at, opcode, ref target) in &self.fixups {
            let address = match target {
                Target::Label(label) => self.labels[label.0].expect("every label is placed"),
                Target::Data(offset) => data_start + offset,
            };
            image[at..at + 3].copy_from_slice(&[
                PFIX << 4 | ((address >> 8) & 0xF) as u8,
                PFIX << 4 | ((address >> 4) & 0xF) as u8,
                opcode << 4 | (address & 0xF) as u8,
            ]);
        }
        image.extend(&self.data);
        if image.len() > SRAM_BYTES {
            return Err(format!(
                "the program needs {} bytes of SRAM ({} code, {} data); the chip has {SRAM_BYTES}",
                image.len(),
                data_start,
                self.data.len()
            ));
        }
        Ok(image)
    }
}
