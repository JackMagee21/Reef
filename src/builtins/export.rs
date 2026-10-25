
use crate::shell::Shell;
use crate::streams::Io;

pub fn export_cmd(sh: &mut Shell, argv: &[String], io: &mut Io) -> i32 {
    let args: &[String] = argv.get(1..).unwrap_or(&[]);
    let failed: bool = false;
    for argument in args {
       let (name, value) = match argument.split_once('=') {
           Some((name, value)) => (name, Some(value)),
           None => (argument.as_str(), None),
       };

       match value {
           Some(v) => sh.set_var(name, v.to_string(), true),
           None => {
            if let Some(current) = sh.var(name) {
                sh.set_var(name, current.to_string(), true);
            }
           }
       }
    }

    if failed { 1 } else { 0 }
}