use crate::{
    gas,
    interpreter::{Interpreter, STACK_LIMIT},
    interpreter_types::{
        Immediates, InterpreterTypes, Jumps, LoopControl, MemoryTr, RuntimeFlag, StackTr,
    },
    InstructionResult, InterpreterAction,
};
use bytecode::opcode;
use primitives::{Bytes, U256};

use crate::InstructionContext;

/// Implements the JUMP instruction.
///
/// Unconditional jump to a valid destination.
pub fn jump<ITy: InterpreterTypes, H: ?Sized>(context: InstructionContext<'_, H, ITy>) {
    //gas!(context.interpreter, gas::MID);
    popn!([target], context.interpreter);
    jump_inner(context.interpreter, target);
}

/// Implements the JUMPI instruction.
///
/// Conditional jump to a valid destination if condition is true.
pub fn jumpi<WIRE: InterpreterTypes, H: ?Sized>(context: InstructionContext<'_, H, WIRE>) {
    //gas!(context.interpreter, gas::HIGH);
    popn!([target, cond], context.interpreter);

    if !cond.is_zero() {
        jump_inner(context.interpreter, target);
    }
}

/// PUSH2 then JUMP or JUMPI, which is how solc emits nearly every jump: runs
/// the pair as one instruction, saving the second dispatch and the push and
/// pop of the target, and returns `true`.  Called by PUSH2 with the
/// instruction pointer on its immediate; returns `false`, having done
/// nothing, when the next opcode is neither or when the pair would halt on
/// the stack (a full stack for the push, or no condition for JUMPI), which
/// the two instructions then do apart.
///
/// The pair charges what the two would: PUSH2's gas is already charged, and
/// JUMP's (8) or JUMPI's (10) is charged here before the jump, as the
/// dispatch of the second instruction would.
#[inline(always)]
pub(crate) fn push2_jump<WIRE: InterpreterTypes>(interpreter: &mut Interpreter<WIRE>) -> bool {
    // The immediate and the opcode after it; the bytecode is padded with
    // zeros past its end.
    let bytes = interpreter.bytecode.read_slice(3);
    let (target, next) = (((bytes[0] as usize) << 8) | bytes[1] as usize, bytes[2]);
    let len = interpreter.stack.len();
    if len >= STACK_LIMIT {
        return false;
    }
    match next {
        opcode::JUMP => {
            if interpreter.gas.record_cost_unsafe(gas::MID) {
                interpreter.halt_oog();
                return true;
            }
            jump_to(interpreter, target);
        }
        opcode::JUMPI if len >= 1 => {
            if interpreter.gas.record_cost_unsafe(gas::HIGH) {
                interpreter.halt_oog();
                return true;
            }
            popn!([cond], interpreter, true);
            if cond.is_zero() {
                interpreter.bytecode.relative_jump(3);
            } else {
                jump_to(interpreter, target);
            }
        }
        _ => return false,
    }
    true
}

/// Internal helper function for jump operations.
///
/// Validates jump target and performs the actual jump.
#[inline(always)]
fn jump_inner<WIRE: InterpreterTypes>(interpreter: &mut Interpreter<WIRE>, target: U256) {
    let target = as_usize_or_fail!(interpreter, target, InstructionResult::InvalidJump);
    jump_to(interpreter, target);
}

/// Jumps to `target`, which must be a JUMPDEST.
#[inline(always)]
fn jump_to<WIRE: InterpreterTypes>(interpreter: &mut Interpreter<WIRE>, target: usize) {
    if !interpreter.bytecode.is_valid_legacy_jump(target) {
        interpreter.halt(InstructionResult::InvalidJump);
        return;
    }
    // The destination is a JUMPDEST, which only costs its gas: charge it
    // here and land past it, which saves its dispatch (one taken jump in
    // every sixteen opcodes of a mainnet block).
    if interpreter.gas.record_cost_unsafe(gas::JUMPDEST) {
        interpreter.halt_oog();
        return;
    }
    // SAFETY: `is_valid_jump` ensures that `dest` is in bounds, and the
    // bytecode is padded with a STOP past its last byte.
    interpreter.bytecode.absolute_jump(target + 1);
}

/// Implements the JUMPDEST instruction.
///
/// Marks a valid destination for jump operations.
pub fn jumpdest<WIRE: InterpreterTypes, H: ?Sized>(_context: InstructionContext<'_, H, WIRE>) {
    //gas!(context.interpreter, gas::JUMPDEST);
}

/// Implements the PC instruction.
///
/// Pushes the current program counter onto the stack.
pub fn pc<WIRE: InterpreterTypes, H: ?Sized>(context: InstructionContext<'_, H, WIRE>) {
    //gas!(context.interpreter, gas::BASE);
    // - 1 because we have already advanced the instruction pointer in `Interpreter::step`
    push!(
        context.interpreter,
        U256::from(context.interpreter.bytecode.pc() - 1)
    );
}

#[inline]
/// Internal helper function for return operations.
///
/// Handles memory data retrieval and sets the return action.
fn return_inner(
    interpreter: &mut Interpreter<impl InterpreterTypes>,
    instruction_result: InstructionResult,
) {
    // Zero gas cost
    // //gas!(interpreter, gas::ZERO)
    popn!([offset, len], interpreter);
    let len = as_usize_or_fail!(interpreter, len);
    // Important: Offset must be ignored if len is zeros
    let mut output = Bytes::default();
    if len != 0 {
        let offset = as_usize_or_fail!(interpreter, offset);
        resize_memory!(interpreter, offset, len);
        output = interpreter.memory.slice_len(offset, len).to_vec().into()
    }

    interpreter
        .bytecode
        .set_action(InterpreterAction::new_return(
            instruction_result,
            output,
            interpreter.gas,
        ));
}

/// Implements the RETURN instruction.
///
/// Halts execution and returns data from memory.
pub fn ret<WIRE: InterpreterTypes, H: ?Sized>(context: InstructionContext<'_, H, WIRE>) {
    return_inner(context.interpreter, InstructionResult::Return);
}

/// EIP-140: REVERT instruction
pub fn revert<WIRE: InterpreterTypes, H: ?Sized>(context: InstructionContext<'_, H, WIRE>) {
    check!(context.interpreter, BYZANTIUM);
    return_inner(context.interpreter, InstructionResult::Revert);
}

/// Stop opcode. This opcode halts the execution.
pub fn stop<WIRE: InterpreterTypes, H: ?Sized>(context: InstructionContext<'_, H, WIRE>) {
    context.interpreter.halt(InstructionResult::Stop);
}

/// Invalid opcode. This opcode halts the execution.
pub fn invalid<WIRE: InterpreterTypes, H: ?Sized>(context: InstructionContext<'_, H, WIRE>) {
    context.interpreter.halt(InstructionResult::InvalidFEOpcode);
}

/// Unknown opcode. This opcode halts the execution.
pub fn unknown<WIRE: InterpreterTypes, H: ?Sized>(context: InstructionContext<'_, H, WIRE>) {
    context.interpreter.halt(InstructionResult::OpcodeNotFound);
}

#[cfg(test)]
mod tests {
    use crate::{
        host::DummyHost,
        instructions::instruction_table,
        interpreter::{EthInterpreter, ExtBytecode, InputsImpl, SharedMemory},
        Interpreter, InstructionResult,
    };
    use bytecode::Bytecode;
    use primitives::{hardfork::SpecId, Bytes, U256};

    /// Runs `code` with `gas_limit`: the result, the gas spent and the stack.
    fn run(code: &[u8], gas_limit: u64) -> (InstructionResult, u64, Vec<U256>) {
        let mut interpreter = Interpreter::<EthInterpreter>::new(
            SharedMemory::new(),
            ExtBytecode::new(Bytecode::new_raw(Bytes::copy_from_slice(code))),
            InputsImpl::default(),
            false,
            SpecId::default(),
            gas_limit,
        );
        let table = instruction_table::<EthInterpreter, DummyHost>();
        let action = interpreter.run_plain(&table, &mut DummyHost);
        let result = action.instruction_result().expect("the frame returned");
        (result, gas_limit - interpreter.gas.remaining(), interpreter.stack.data().to_vec())
    }

    const U: fn(u64) -> U256 = U256::from;

    #[test]
    fn push2_jump_lands_past_the_jumpdest() {
        // PUSH2 5, JUMP, INVALID, JUMPDEST, PUSH1 1, STOP
        let code = [0x61, 0x00, 0x05, 0x56, 0xfe, 0x5b, 0x60, 0x01, 0x00];
        assert_eq!(run(&code, 100), (InstructionResult::Stop, 3 + 8 + 1 + 3, vec![U(1)]));
    }

    #[test]
    fn out_of_gas_on_the_folded_jumpdest() {
        let code = [0x61, 0x00, 0x05, 0x56, 0xfe, 0x5b, 0x60, 0x01, 0x00];
        // 11 pays PUSH2 and JUMP but not the JUMPDEST, 14 not the PUSH1 after it
        for limit in [3, 10, 11, 12, 14] {
            assert_eq!(run(&code, limit).0, InstructionResult::OutOfGas, "gas limit {limit}");
            assert_eq!(run(&code, limit).1, limit, "gas limit {limit}");
        }
        assert_eq!(run(&code, 15).0, InstructionResult::Stop);
    }

    #[test]
    fn push2_jump_to_a_non_jumpdest_halts() {
        // PUSH2 4 (the INVALID), JUMP
        let code = [0x61, 0x00, 0x04, 0x56, 0xfe, 0x5b, 0x00];
        assert_eq!(run(&code, 100).0, InstructionResult::InvalidJump);
        // PUSH2 7, a 0x5b inside the operand of the PUSH2 at 5
        let code = [0x61, 0x00, 0x07, 0x56, 0x5b, 0x61, 0x5b, 0x5b, 0x00];
        assert_eq!(run(&code, 100).0, InstructionResult::InvalidJump);
        // past the end
        let code = [0x61, 0x01, 0x00, 0x56, 0x5b, 0x00];
        assert_eq!(run(&code, 100).0, InstructionResult::InvalidJump);
    }

    #[test]
    fn push2_jumpi_not_taken_runs_on() {
        // PUSH1 0, PUSH2 7, JUMPI, STOP, JUMPDEST, STOP
        let code = [0x60, 0x00, 0x61, 0x00, 0x07, 0x57, 0x00, 0x5b, 0x00];
        assert_eq!(run(&code, 100), (InstructionResult::Stop, 3 + 3 + 10, vec![]));
        // a not-taken JUMPI does not check its target
        let code = [0x60, 0x00, 0x61, 0x00, 0x06, 0x57, 0x60, 0x02, 0x00];
        assert_eq!(run(&code, 100), (InstructionResult::Stop, 3 + 3 + 10 + 3, vec![U(2)]));
    }

    #[test]
    fn push2_jumpi_taken() {
        // PUSH1 1, PUSH2 8, JUMPI, INVALID, INVALID, JUMPDEST, PUSH1 2, STOP
        let code = [0x60, 0x01, 0x61, 0x00, 0x08, 0x57, 0xfe, 0xfe, 0x5b, 0x60, 0x02, 0x00];
        assert_eq!(run(&code, 100), (InstructionResult::Stop, 3 + 3 + 10 + 1 + 3, vec![U(2)]));
        assert_eq!(run(&code, 16).0, InstructionResult::OutOfGas);
        assert_eq!(run(&code, 15).0, InstructionResult::OutOfGas);
    }

    #[test]
    fn push2_jumpi_without_a_condition_underflows() {
        let code = [0x61, 0x00, 0x04, 0x57, 0x5b, 0x00];
        assert_eq!(run(&code, 100).0, InstructionResult::StackUnderflow);
    }

    #[test]
    fn pc_after_a_folded_jumpdest() {
        // PUSH2 5, JUMP, INVALID, JUMPDEST, PC, STOP
        let code = [0x61, 0x00, 0x05, 0x56, 0xfe, 0x5b, 0x58, 0x00];
        assert_eq!(run(&code, 100), (InstructionResult::Stop, 3 + 8 + 1 + 2, vec![U(6)]));
    }

    #[test]
    fn computed_jumps_fold_too() {
        // PUSH1 4, JUMP, INVALID, JUMPDEST, STOP; PUSH1 1, PUSH1 6, JUMPI, ...
        let code = [0x60, 0x04, 0x56, 0xfe, 0x5b, 0x00];
        assert_eq!(run(&code, 100), (InstructionResult::Stop, 3 + 8 + 1, vec![]));
        let code = [0x60, 0x01, 0x60, 0x07, 0x57, 0xfe, 0xfe, 0x5b, 0x00];
        assert_eq!(run(&code, 100), (InstructionResult::Stop, 3 + 3 + 10 + 1, vec![]));
    }

    #[test]
    fn push2_jump_on_a_full_stack_overflows() {
        for (pushes, result) in [(1023, InstructionResult::Stop), (1024, InstructionResult::StackOverflow)] {
            let target = pushes + 4;
            let mut code = vec![0x5f; pushes];
            code.extend([0x61, (target >> 8) as u8, target as u8, 0x56, 0x5b, 0x00]);
            assert_eq!(run(&code, 1_000_000).0, result, "{pushes} pushes");
        }
        // JUMPI too: 1023 conditions of 1, then PUSH2 JUMPI
        let mut code = vec![0x60, 0x01].repeat(1023);
        let target = code.len() + 4;
        code.extend([0x61, (target >> 8) as u8, target as u8, 0x57, 0x5b, 0x00]);
        assert_eq!(run(&code, 1_000_000).0, InstructionResult::Stop);
        code.splice(0..0, [0x60, 0x01]);
        let target = code.len() - 2;
        let n = code.len();
        code[n - 5] = (target >> 8) as u8;
        code[n - 4] = target as u8;
        assert_eq!(run(&code, 1_000_000).0, InstructionResult::StackOverflow);
    }
}
