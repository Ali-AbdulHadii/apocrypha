# Activate the project-contained toolchains, for fish. Source it, don't run it:
#
#   source env.fish
#
# The bash version of this is env.sh. Both exist because fish is not a POSIX
# shell: `export VAR=value` is a syntax error in it, so sourcing env.sh from
# fish fails on its first line without setting anything, and the failure reads
# like a broken toolchain rather than a wrong file.

set -l here (dirname (status --current-filename))
set -l root (cd $here; and pwd)

set -gx RUSTUP_HOME $root/.rustup
set -gx CARGO_HOME $root/.cargo

# Prepend so the contained toolchain wins over anything installed system-wide,
# and so re-sourcing does not stack duplicates onto PATH.
fish_add_path --global --prepend --move $root/.cargo/bin $root/.node/bin

# npm is a Node script with a `#!/usr/bin/env node` shebang, so node has to be
# on PATH before npm will run at all — which is why .node/bin is added rather
# than the binaries being invoked by absolute path.

echo "rust:  "(rustc --version 2>/dev/null; or echo 'not installed')
echo "cargo: "(cargo --version 2>/dev/null; or echo 'not installed')
echo "node:  "(node --version 2>/dev/null; or echo 'not installed')
