{ phase }:
derivation {
  name = "bootc-vm-${phase}-test";
  system = builtins.currentSystem;
  builder = "/bin/sh";
  args = [
    "-c"
    "echo ${phase}-ok > $out"
  ];
}
