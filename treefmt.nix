# treefmt configuration for nix-installer
{ pkgs, ... }:
{
  # Used to find the project root
  projectRootFile = "flake.nix";

  # Rust formatting
  programs.rustfmt = {
    enable = true;
    edition = "2024";
  };

  # Nix formatting (nixfmt-rfc-style is the new standard)
  programs.nixfmt = {
    enable = true;
    package = pkgs.nixfmt-rfc-style;
  };

  # Shell script formatting and linting
  programs.shfmt.enable = true;
  programs.shellcheck.enable = true;
  settings.formatter.shellcheck.excludes = [
    ".envrc" # direnv file, not a regular shell script
  ];
  settings.formatter.shfmt.excludes = [
    ".envrc"
  ];

  # Spell checking (config in _typos.toml)
  programs.typos.enable = true;

  # TOML formatting
  programs.taplo.enable = true;

  # YAML formatting
  programs.yamlfmt = {
    enable = true;
    settings.formatter = {
      retain_line_breaks = true;
    };
  };

  # Action linting for GitHub workflows
  programs.actionlint.enable = true;

  # Editorconfig checking
  settings.formatter.editorconfig-checker = {
    command = pkgs.lib.getExe pkgs.editorconfig-checker;
    includes = [ "*" ];
    excludes = [
      "*.lock"
      "target/*"
      "*.pp" # selinux binary policy
    ];
    priority = 1; # Run after formatters
  };
}
