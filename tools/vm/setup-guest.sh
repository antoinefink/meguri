#!/bin/bash
# One-time provisioning of the meguri-mac test VM (docs/process/mac-ui-vm.md),
# authorized by Antoine on 2026-10-15. Copy this script and the host's VM
# public key (as meguri-vm.pub) into a folder shared with the VM, then run it
# in the VM's Terminal, signed in as the VM account. It changes the VM only,
# never the host. It asks for the VM account's password (sudo, screen lock,
# auto-login) and asks before it accepts the Xcode license. Copy Xcode into
# the VM with tar over SSH instead of the shared folder, which breaks its
# symlinks.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
shared="$(dirname "$here")"
me="$(id -un)"
step() { printf '\n== %s\n' "$1"; }

echo "Setting up this VM (not your Mac) for Meguri's Mac UI tests, as user '$me'."
sudo -v

step "Passwordless sudo for $me"
echo "$me ALL=(ALL) NOPASSWD: ALL" | sudo tee /etc/sudoers.d/meguri-vm >/dev/null
sudo chmod 440 /etc/sudoers.d/meguri-vm
sudo visudo -cf /etc/sudoers.d/meguri-vm

step "SSH login with the host's VM key only"
mkdir -p ~/.ssh && chmod 700 ~/.ssh
touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys
grep -qxF "$(cat "$here/meguri-vm.pub")" ~/.ssh/authorized_keys || cat "$here/meguri-vm.pub" >> ~/.ssh/authorized_keys
printf 'PasswordAuthentication no\nKbdInteractiveAuthentication no\n' | sudo tee /etc/ssh/sshd_config.d/100-meguri-vm.conf >/dev/null
sudo launchctl enable system/com.openssh.sshd 2>/dev/null || true
sudo launchctl bootstrap system /System/Library/LaunchDaemons/ssh.plist 2>/dev/null || true
sudo launchctl kickstart -k system/com.openssh.sshd 2>/dev/null || true
sleep 2
if ! nc -z 127.0.0.1 22 2>/dev/null; then
  echo "Remote Login did not start. Turn it on in System Settings > General > Sharing > Remote Login, then run this script again."
  exit 1
fi

step "No sleep, screen lock or screen saver"
sudo pmset -a sleep 0 displaysleep 0 disksleep 0
defaults -currentHost write com.apple.screensaver idleTime -int 0
echo "Type this VM account's password to turn off the screen lock:"
sysadminctl -screenLock off -password -

step "Automatic login at boot"
echo "Type this VM account's password once more for automatic login:"
sudo sysadminctl -autologin set -userName "$me" -password -

step "Developer mode and UI automation without authentication"
sudo DevToolsSecurity -enable
sudo automationmodetool enable-automationmode-without-authentication

step "Xcode 27 (copied from the host)"
if [ ! -d /Applications/Xcode.app ]; then
  echo "Copying Xcode (a few minutes)..."
  sudo ditto "$shared/xcode" /Applications/Xcode.app
fi
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
read -r -p "Accept the Xcode and Apple SDKs license in this VM? You already accepted it on your Mac. [y/N] " ok
case "$ok" in
  y|Y|yes|YES) sudo xcodebuild -license accept ;;
  *) echo "Not accepted. Run 'sudo xcodebuild -license accept' in this VM later; UI tests cannot run until then."; exit 1 ;;
esac
sudo xcodebuild -runFirstLaunch

step "Done"
echo "VM address: $(ipconfig getifaddr en0 || echo unknown). You can close Terminal; Claude takes it from here."
