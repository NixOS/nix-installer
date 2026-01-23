# treefmt configuration for nix-installer
{ pkgs, ... }:
{
  # Used to find the project root
  projectRootFile = "flake.nix";

  # Rust formatting
  programs.rustfmt = {
    enable = true;
  };

  # Nix formatting (nixfmt-rfc-style is the new standard)
  programs.nixfmt = {
    enable = true;
    package = pkgs.nixfmt-rfc-style;
  };

  # Shell script formatting and linting
  programs.shfmt.enable = true;
  programs.shellcheck.enable = true;

  # Spell checking
  programs.typos = {
    enable = true;
  };
  settings.formatter.typos = {
    excludes = [
      "*.lock"
      "target/*"
      "src/action/linux/selinux/*"
    ];
  };

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
