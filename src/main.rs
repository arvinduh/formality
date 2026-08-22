fn main() {
  let exit_status = fml::run();
  std::process::exit(exit_status.code());
}
