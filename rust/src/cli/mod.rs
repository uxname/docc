use std::io::Write;

pub fn run(_args: &[String], _stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = writeln!(stderr, "docc: not ported yet");
    2
}
