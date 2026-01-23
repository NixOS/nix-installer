FROM default
COPY nix-installer /nix-installer
RUN chmod +x /nix-installer/bin/nix-installer
RUN /nix-installer/bin/nix-installer install linux --logger pretty --log-directive nix_installer=trace --init none --extra-conf "sandbox = false" --no-confirm -vvv
ENV PATH="${PATH}:/nix/var/nix/profiles/default/bin"
RUN nix-build --no-substitute -E 'derivation { name = "foo"; system = "x86_64-linux"; builder = "/bin/sh"; args = ["-c" "echo foobar > $out"]; }'
RUN /nix/nix-installer uninstall --no-confirm
