# GitHub Actions Self-Hosted Runner Setup

This document explains how to set up the GitHub Actions runner on your server for automatic deployments.

## Prerequisites

- Server: 23.88.88.105
- Runner user with sudo privileges
- Rust toolchain installed

## 1. Install GitHub Actions Runner

On your server (23.88.88.105):

```bash
# Create runner directory
mkdir -p ~/actions-runner
cd ~/actions-runner

# Download latest runner (replace with current version)
curl -o actions-runner-linux-x64-2.311.0.tar.gz -L \
  https://github.com/actions/runner/releases/download/v2.311.0/actions-runner-linux-x64-2.311.0.tar.gz

# Extract
tar xzf ./actions-runner-linux-x64-2.311.0.tar.gz

# Configure (get token from GitHub repo settings)
./config.sh --url https://github.com/samansohani78/SNIProxy-rs \
  --token YOUR_TOKEN \
  --name sniproxy \
  --labels sniproxy \
  --work _work

# Install as service
sudo ./svc.sh install

# Start the service
sudo ./svc.sh start
```

**Get the token from:**
https://github.com/samansohani78/SNIProxy-rs/settings/actions/runners/new

## 2. Configure Passwordless Sudo

The runner needs sudo permissions for deployment. Create a sudoers file:

```bash
# Edit sudoers (replace YOUR_RUNNER_USER with actual username)
sudo visudo -f /etc/sudoers.d/github-runner
```

Add these lines (replace `YOUR_RUNNER_USER` with the actual runner username):

```
# GitHub Actions Runner - SNIProxy deployment permissions
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/systemctl stop sniproxy
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/systemctl start sniproxy
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/systemctl enable sniproxy
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/systemctl daemon-reload
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/systemctl status sniproxy
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/systemctl is-active sniproxy
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/cp * /usr/local/bin/
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/cp * /usr/local/bin/sniproxy-server
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/chmod +x /usr/local/bin/sniproxy-server
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /bin/mkdir -p /etc/sniproxy
YOUR_RUNNER_USER ALL=(ALL) NOPASSWD: /usr/bin/bash */install.sh
```

Set correct permissions:

```bash
sudo chmod 0440 /etc/sudoers.d/github-runner
```

## 3. Verify Rust Installation

The runner needs Rust toolchain:

```bash
# Check if Rust is installed
rustc --version
cargo --version

# If not installed, install it
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

## 4. Create Working Directory

```bash
# Create directory for builds
mkdir -p ~/actions-runner/_work/SNIProxy-rs/SNIProxy-rs
```

## 5. Test Runner

```bash
# Check runner status
sudo ./svc.sh status

# View runner logs
journalctl -u actions.runner.* -f
```

## 6. Verify GitHub Connection

1. Go to: https://github.com/samansohani78/SNIProxy-rs/settings/actions/runners
2. You should see runner "sniproxy" with status "Idle"
3. Tag should show: "sniproxy"

## 7. Test Deployment

Make a small change and push:

```bash
# On your local machine
echo "# Test" >> README.md
git add README.md
git commit -m "test: trigger CI/CD"
git push origin main
```

Watch the workflow:
https://github.com/samansohani78/SNIProxy-rs/actions

## Workflow Behavior

### On Push to Main Branch

The workflow will:

1. **Run fast checks** (GitHub-hosted runners):
   - Format check
   - Clippy lints
   - Security audit

2. **Build and test** (your server):
   - Build debug
   - Run tests
   - Build release
   - Run release tests
   - Protocol verification

3. **Deploy** (your server):
   - Stop current service
   - Install new binary
   - Configure systemd
   - Start service
   - Verify deployment

### On Pull Requests

The workflow will:
- Run all checks
- Build and test
- **NOT deploy** (only on main branch)

## Troubleshooting

### Runner Not Showing Up

```bash
# Check runner service
sudo systemctl status actions.runner.*

# Restart runner
sudo ./svc.sh stop
sudo ./svc.sh start
```

### Sudo Permission Denied

```bash
# Test sudo commands
sudo systemctl status sniproxy
sudo cp /tmp/test /usr/local/bin/

# If fails, verify sudoers file
sudo cat /etc/sudoers.d/github-runner
```

### Build Fails

```bash
# Check Rust installation
rustc --version
cargo --version

# Check disk space
df -h

# Check runner logs
journalctl -u actions.runner.* -f
```

### Deployment Fails

```bash
# Check if binary exists
ls -l ~/actions-runner/_work/SNIProxy-rs/SNIProxy-rs/target/release/sniproxy-server

# Check service status
sudo systemctl status sniproxy

# View service logs
sudo journalctl -u sniproxy -f
```

## Security Notes

1. **Runner Isolation**: The runner runs under a dedicated user account
2. **Limited Sudo**: Only specific commands are allowed via sudoers
3. **GitHub Token**: Stored securely by GitHub Actions
4. **Self-Hosted**: Runs on your infrastructure, no code sent to GitHub runners

## Maintenance

### Update Runner

```bash
cd ~/actions-runner
sudo ./svc.sh stop
# Download and extract new version
sudo ./svc.sh start
```

### Remove Runner

```bash
cd ~/actions-runner
sudo ./svc.sh stop
sudo ./svc.sh uninstall
./config.sh remove --token YOUR_REMOVAL_TOKEN
```

## Support

- GitHub Actions Docs: https://docs.github.com/en/actions
- Runner Docs: https://docs.github.com/en/actions/hosting-your-own-runners
- Repository Actions: https://github.com/samansohani78/SNIProxy-rs/actions
