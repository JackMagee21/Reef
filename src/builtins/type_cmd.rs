use crate::platform::find_command;
use crate::shell::Shell;
use crate::streams::Io;

pub fn type_cmd(sh: &mut Shell, argv: &[String], io: &mut Io) -> i32 {
    let mut failed = false;
    for name in argv.get(1..).unwrap_or(&[]) {
        let status = if super::lookup(name).is_some() {
            super::print(io, "type", &format!("{name} is a shell builtin\n"))
        } else if let Some(path) = find_command(name, sh) {
            super::print(io, "type", &format!("{name} is {}\n", path.display()))
        } else {
            io.error(format_args!("type: {name}: not found"));
            1
        };
        failed |= status != 0;
    }
    if failed { 1 } else { 0 }
}
