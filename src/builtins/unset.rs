
use crate::shell::Shell;
use crate::streams::Io;

pub fn unset(sh: &mut Shell, argv: &[String], _io: &mut Io) -> i32 {
    let args: &[String] = argv.get(1..).unwrap_or(&[]);
    for arg in args {
        sh.unset_var(arg); // Unsets variable name
    }
    return 0;

}