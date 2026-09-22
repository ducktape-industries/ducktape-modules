use std::{env, fs, path::Path, process::ExitCode};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let (output, bytes) = match args.as_slice() {
        [flag, program, output] if flag == "--strip" => {
            (output, view_pack::strip(&fs::read(program)?)?)
        }
        [program, view, output] => (
            output,
            view_pack::embed(&fs::read(program)?, &fs::read(view)?)?,
        ),
        _ => return Err("usage: view-pack PROGRAM.wasm VIEW.wasm OUTPUT.wasm\n       view-pack --strip PROGRAM.wasm OUTPUT.wasm".into()),
    };
    // Read and validate both inputs before touching the output; in-place use is supported.
    if fs::read(Path::new(output)).ok().as_deref() != Some(&bytes) {
        fs::write(output, bytes)?;
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("view-pack: {error}");
            ExitCode::FAILURE
        }
    }
}
