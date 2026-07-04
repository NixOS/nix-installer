FROM default
COPY nix-installer /nix-installer
RUN chmod +x /nix-installer/bin/nix-installer
# In a container build there is no systemd, so we use the bootc planner which
# installs Nix into /nix (part of the image layer). On first boot,
# systemd-tmpfiles copies it to /var/lib/nix and a bind-mount exposes it.
RUN /nix-installer/bin/nix-installer install bootc --logger pretty --log-directive nix_installer=trace --extra-conf "sandbox = false" --no-confirm -vvv
ENV PATH="${PATH}:/nix/var/nix/profiles/default/bin"
RUN nix-build --no-substitute -E 'derivation { name = "foo"; system = "x86_64-linux"; builder = "/bin/sh"; args = ["-c" "echo foobar > $out"]; }'
# Verify build-time artifacts:
RUN test -d /nix/store || { echo "FAIL: /nix/store does not exist"; exit 1; }
RUN test -f /etc/sysusers.d/nix-installer.conf || { echo "FAIL: /etc/sysusers.d/nix-installer.conf missing"; exit 1; }
RUN test -f /etc/tmpfiles.d/nix-installer.conf || { echo "FAIL: /etc/tmpfiles.d/nix-installer.conf missing"; exit 1; }
RUN grep -q 'After=systemd-tmpfiles-setup.service' /etc/systemd/system/nix.mount || { echo "FAIL: nix.mount missing After=systemd-tmpfiles-setup.service"; cat /etc/systemd/system/nix.mount; exit 1; }
