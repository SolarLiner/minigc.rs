use tracing_flame::FlameLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::interpreter::{Instruction, Interpreter};

mod interpreter;
mod traits;
mod vm;

fn setup_tracing() -> impl Drop {
    let fmt_layer = tracing_subscriber::fmt::Layer::new()
        .with_ansi(true)
        .pretty();
    let (flame_layer, _guard) = FlameLayer::with_file("./gc.trace").unwrap();

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(flame_layer)
        .init();
    _guard
}

fn main() -> anyhow::Result<()> {
    let _guard = setup_tracing();
    let mut interpreter = Interpreter::new(vec![
        Instruction::ConstInt(3),
        Instruction::ConstInt(7),
        Instruction::ConstInt(2),
        Instruction::IMul,
        Instruction::IMul,
    ])
    .unwrap();
    let top = interpreter.run()?;
    println!("Value (should be 42): {}", interpreter.display(top)?);
    Ok(())
}
