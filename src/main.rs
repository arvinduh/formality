//! `fml` binary entry point — thin wrapper delegating to [`fml::run`], which
//! owns argument parsing and command dispatch.

fn main() {
  let exit_status = fml::run();
  std::process::exit(exit_status.code());
}
