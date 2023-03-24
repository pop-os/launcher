ID := 'pop-launcher'
plugins := 'calc desktop_entries files find pop_shell pulse recent scripts terminal web cosmic_toplevel'

rootdir := ''

base-dir := if rootdir == '' {
    env_var('HOME') / '.local'
} else {
    rootdir / 'usr'
}

lib-dir := if rootdir == '' {
    base-dir / 'share'
} else {
    base-dir / 'lib'
}

bin-dir := base-dir / 'bin'
bin-path := bin-dir / ID

launcher-dir := lib-dir / ID
scripts-dir := launcher-dir / 'scripts/'
plugin-dir := launcher-dir / 'plugins/'

version := '0.0.0'

# Compile pop-launcher
all *args: (build-release args)

# Compile with debug profile
build-debug *args:
    cargo build -p pop-launcher-bin {{args}}

# Compile with release profile
build-release *args: (build-debug '--release' args)

# Compile with a vendored tarball
build-vendored *args: _vendor-extract (build-release '--frozen --offline' args)

# Check for errors and linter warnings
check *args:
    cargo clippy --all-features {{args}} -- -W clippy::pedantic

# Runs a check with JSON message format for IDE integration
check-json: (check '--message-format=json')

# Remove Cargo build artifacts
clean:
    cargo clean

# Also remove .cargo and vendored dependencies
clean-dist:
    rm -rf .cargo vendor vendor.tar target

# Install everything
install: install-bin install-plugins install-scripts

# Install pop-launcher binary
install-bin:
    install -Dm0755 target/release/pop-launcher-bin {{bin-path}}

# Install pop-launcher plugins
install-plugins:
    #!/usr/bin/env sh
    set -ex
    for plugin in {{plugins}}; do
        dest={{plugin-dir}}${plugin}
        mkdir -p ${dest}
        install -Dm0644 plugins/src/${plugin}/*.ron ${dest}
        ln -sf {{bin-path}} {{plugin-dir}}${plugin}/$(echo ${plugin} | sed 's/_/-/')
    done

# Install pop-launcher scripts
install-scripts:
    #!/usr/bin/env sh
    set -ex
    mkdir -p {{scripts-dir}}
    for script in {{justfile_directory()}}/scripts/*; do
        cp -r ${script} {{scripts-dir}}
    done

# Uninstalls everything (requires same arguments as given to install)
uninstall:
    rm {{bin-path}}
    rm -rf {{launcher-dir}}

# Vendor Cargo dependencies locally
vendor:
    mkdir -p .cargo
    cargo vendor --sync bin/Cargo.toml \
        --sync plugins/Cargo.toml \
        --sync service/Cargo.toml \
        | head -n -1 > .cargo/config
    echo 'directory = "vendor"' >> .cargo/config
    tar pcf vendor.tar vendor
    rm -rf vendor

# Extracts vendored dependencies if vendor=1
_vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar
