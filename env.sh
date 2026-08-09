# Activate the project-contained toolchains. Source it, don't run it:
#
#   source env.sh
#
# Rust and Node live inside this repository rather than on the machine, so a
# build does not depend on what happens to be installed globally and cannot be
# broken by a system upgrade. `rust-toolchain.toml` still pins the channel, so
# rustup resolves the same compiler either way.

APOC_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"

export RUSTUP_HOME="$APOC_ROOT/.rustup"
export CARGO_HOME="$APOC_ROOT/.cargo"
export PATH="$CARGO_HOME/bin:$APOC_ROOT/.node/bin:$PATH"

# npm is a Node script with a `#!/usr/bin/env node` shebang, so it needs node on
# PATH before it will run at all — which is why .node/bin goes on PATH rather
# than the binaries being called by absolute path.

echo "rust:  $(rustc --version 2>/dev/null || echo 'not installed')"
echo "cargo: $(cargo --version 2>/dev/null || echo 'not installed')"
echo "node:  $(node --version 2>/dev/null || echo 'not installed')"
